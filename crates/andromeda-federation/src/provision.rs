//! Federated single-entity provision operations (clauses 4.3.6, 5.6, 6.3.17),
//! on the current entity or on its temporal evolution.

use crate::{
    FederationService,
    attribute_coverage::{matching_attributes, narrowed_document},
    error::FederationError,
    forwarded_content::ForwardedContent,
    forwarded_names::ForwardedNames,
    forwarded_write::{PlannedBody, PlannedContent, PlannedWrite},
    forwarding_context::BodyPresence,
    forwarding_url::forwarding_url,
    operation::ProvisionResource,
    outcome::{ForwardedWriteOutcome, ForwardedWriteResult},
    provision_plan::{ProvisionPlan, RegistrationSelection},
    registration_payload::forwarding_decision,
    request::ForwardProvisionRequest,
    term_resolver::TermResolver,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_model::registration::{ContextSourceRegistration, forwarding_mode::ForwardingMode};
use andromeda_ngsi_ld_render::version::adapted_payload::AdaptedPayload;
use iri_rs::IriBuf;
use std::borrow::Cow;
use url::Url;

/// What decides which attributes of one write a registration covers.
#[derive(Clone, Copy)]
struct WriteCoverage<'a> {
    /// The entity types the registrations were matched on.
    entity_types: &'a [IriBuf],
    /// The attribute the request path names, by its expanded name.
    expanded_attribute: Option<&'a AttributeName>,
    /// The expanded names of the attributes the content writes.
    requested: &'a [AttributeName],
}

impl FederationService {
    /// Forward a single NGSI-LD provision operation carrying `content`.
    ///
    /// # Errors
    ///
    /// As [`FederationService::plan_provision`]. A source that rejects the
    /// write does not fail the call: its rejection is reported as one of the
    /// returned [`ForwardedWriteResult`]s.
    pub async fn forward_provision(
        &self,
        target: &ContextTarget,
        request: ForwardProvisionRequest<'_>,
        content: Option<ForwardedContent<'_>>,
    ) -> Result<Vec<ForwardedWriteResult>, FederationError> {
        let plan = self.plan_provision(target, &request).await?;
        self.execute_provision(plan, &request, content).await
    }

    /// Send a planned provision operation whose admitted content is `content`.
    ///
    /// # Errors
    ///
    /// Returns [`FederationError::JsonLd`] when the request's own names cannot
    /// be spelled for a registration. A source that rejects the write does
    /// not fail the call: its rejection is reported as one of the returned
    /// [`ForwardedWriteResult`]s.
    pub async fn execute_provision(
        &self,
        plan: ProvisionPlan,
        request: &ForwardProvisionRequest<'_>,
        content: Option<ForwardedContent<'_>>,
    ) -> Result<Vec<ForwardedWriteResult>, FederationError> {
        let ProvisionPlan { timer, selection } = plan;
        let result = self.send_provision(selection, request, content).await;
        timer.finish_result(&result);
        result
    }

    /// Send `request` to every registration of `selection`.
    async fn send_provision(
        &self,
        selection: RegistrationSelection,
        request: &ForwardProvisionRequest<'_>,
        content: Option<ForwardedContent<'_>>,
    ) -> Result<Vec<ForwardedWriteResult>, FederationError> {
        let RegistrationSelection {
            mut settled,
            receiving,
            not_advertising,
            entity_types,
            expanded_attribute,
        } = selection;
        if receiving.is_empty() && not_advertising.is_empty() {
            return Ok(settled);
        }
        let resolver = TermResolver::new(&self.engine, request.context);
        let requested = content.as_ref().map(ForwardedContent::written_attributes).unwrap_or_default();
        for csr in not_advertising {
            // Clauses 5.6.1.4 and following: a proxied registration that
            // does not support the operation still owns its attributes, so
            // the write fails for them instead of landing here.
            let proxied_attrs = matching_attributes(&csr, &entity_types, Some(request.entity_id), expanded_attribute.as_ref(), &requested);
            let mode = proxied_mode(&csr);
            settled.push(ForwardedWriteResult {
                registration_id: csr.id,
                mode,
                outcome: ForwardedWriteOutcome::NotAdvertised,
                proxied_attrs,
            });
        }

        let mut planned: Vec<PlannedWrite<'_>> = Vec::new();
        for csr in receiving {
            let coverage = WriteCoverage {
                entity_types: &entity_types,
                expanded_attribute: expanded_attribute.as_ref(),
                requested: &requested,
            };
            if let Some(write) = self.plan_write(csr, request, content, coverage, &resolver).await {
                planned.push(write);
            }
        }

        settled.extend(self.execute_writes(planned).await);
        Ok(settled)
    }

    /// Plan the one forwarded write to `csr` for `request`, whose admitted
    /// content is `content`, or `None` when the registration covers nothing of
    /// the content once it is narrowed to what the registration declares
    /// (clause 4.3.6.1).
    async fn plan_write<'a>(
        &self,
        csr: ContextSourceRegistration,
        request: &ForwardProvisionRequest<'a>,
        content: Option<ForwardedContent<'a>>,
        coverage: WriteCoverage<'_>,
        resolver: &TermResolver<'_>,
    ) -> Option<PlannedWrite<'a>> {
        let presence = match content {
            Some(_) => BodyPresence::Present,
            None => BodyPresence::Absent,
        };
        let url = self.write_url(&csr, request, presence, resolver).await;

        let proxied_attrs = if is_proxied(&csr) {
            matching_attributes(
                &csr,
                coverage.entity_types,
                Some(request.entity_id),
                coverage.expanded_attribute,
                coverage.requested,
            )
        } else {
            Vec::new()
        };

        let planned = match content {
            None => None,
            Some(ForwardedContent::Entity(document)) if request.attribute_name.is_none() => Some(PlannedContent::Entity(Box::new(narrowed_document(
                document,
                &csr,
                coverage.entity_types,
                Some(request.entity_id),
            )?))),
            // The Scope history follows the evolution to every registration
            // that receives it, as the Scope paths of an entity do.
            Some(ForwardedContent::Evolution { document, scope_history }) => Some(PlannedContent::Evolution {
                document: Box::new(narrowed_document(document, &csr, coverage.entity_types, Some(request.entity_id))?),
                scope_history,
            }),
            // The attribute a request path names is written alone (clauses 5.6.4, 5.6.14 and 5.6.19).
            Some(ForwardedContent::Entity(document)) => Some(PlannedContent::Entity(Box::new(Cow::Borrowed(document)))),
            Some(ForwardedContent::Attribute { name, instance }) => Some(PlannedContent::Attribute { name, instance }),
        };
        // A temporal body's attribute arrays are the clause 4.5.7 sequences of
        // instances in time, which the clause 4.3.6.8 fallback reads apart
        // from an entity's sets of instances.
        let element = match request.resource {
            ProvisionResource::Entities => AdaptedPayload::Entity,
            ProvisionResource::TemporalEntities => AdaptedPayload::TemporalEvolution,
        };
        Some(PlannedWrite {
            registration: csr,
            url,
            method: request.method.clone(), // clone: each planned write owns the method it sends
            body: planned.map(|content| PlannedBody { content, element }),
            proxied_attrs,
            context: request.context,
            triggering: request.triggering,
            // A single-entity provision runs under no Entity Map.
            linked_map: None,
        })
    }

    /// The URL one forwarded write to `csr` is sent to, with the attribute its
    /// path names and every name-bearing query parameter spelled in the
    /// registration's forwarding context (clause 4.3.6.6), or the failure to
    /// prepare it, which the write then reports as its own.
    async fn write_url(
        &self,
        csr: &ContextSourceRegistration,
        request: &ForwardProvisionRequest<'_>,
        presence: BodyPresence,
        resolver: &TermResolver<'_>,
    ) -> Result<Url, FederationError> {
        let decision = forwarding_decision(csr, request.context, presence)?;
        let naming = decision.context.naming(request.context);
        let names = ForwardedNames::prepare(&self.engine, resolver, &naming, decision.context.rewording()).await?;
        let spelled_attribute = match request.attribute_name {
            Some(attribute) => Some(names.attr(attribute).await?),
            None => None,
        };
        // A POST below the bare resource creates (clauses 6.4.3.1 and
        // 6.18.3.1); every other write addresses the entity itself, and the
        // attribute segment of its path is spelled for the source.
        let mut path = request.resource.segments().to_vec();
        if !(request.method == reqwest::Method::POST && request.path_suffix.is_none()) {
            path.push(request.entity_id.as_str());
            path.extend(spelled_suffix(request.path_suffix, spelled_attribute.as_ref()));
        }
        let query_string = match request.query_string {
            Some(raw) => Some(names.query_string(raw).await?),
            None => None,
        };
        forwarding_url(csr.access.endpoint.url(), &path, query_string.as_deref())
    }
}

/// The collection segment an attribute path hangs below (clauses 6.6 to 6.7
/// and 6.20 to 6.22).
const ATTRIBUTES_SEGMENT: &str = "attrs";

/// The segments of a path suffix below the entity, with the attribute segment
/// -- the one right after the `attrs` collection -- replaced by `spelled`.
///
/// The attribute is found by its position rather than by its name, so an
/// attribute named like a collection (`attrs/attrs`) replaces only itself.
fn spelled_suffix<'a>(suffix: Option<&'a str>, spelled: Option<&'a AttributeName>) -> Vec<&'a str> {
    let mut segments = suffix
        .into_iter()
        .flat_map(|suffix| suffix.split('/'))
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();
    let attribute_position = segments
        .iter()
        .position(|segment| *segment == ATTRIBUTES_SEGMENT)
        .map(|collection| collection + 1);
    if let (Some(spelled), Some(position)) = (spelled, attribute_position)
        && let Some(segment) = segments.get_mut(position)
    {
        *segment = spelled.as_str();
    }
    segments
}

/// Whether a registration is proxied (clause 4.3.6.3): its data is held
/// outside this broker, so the attributes it covers never stay local.
pub(crate) const fn is_proxied(csr: &ContextSourceRegistration) -> bool {
    matches!(csr.access.mode, Some(ForwardingMode::Exclusive | ForwardingMode::Redirect))
}

/// The mode of a proxied registration; inclusive is the clause 5.2.9 default
/// and never proxied, so it is only reached for a registration `is_proxied`
/// already accepted.
pub(crate) fn proxied_mode(csr: &ContextSourceRegistration) -> ForwardingMode {
    csr.access.mode.unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use crate::{
        FederationService,
        fixed_registration_store::offline_service,
        forwarded_content::ForwardedContent,
        operation::{ForwardingOperation, ProvisionResource},
        outcome::{ForwardedWriteOutcome, ForwardedWriteResult},
        provision::spelled_suffix,
        request::{ForwardProvisionRequest, TriggeringRequest},
    };
    use andromeda_context_space::context_target::ContextTarget;
    use andromeda_jsonld::context_spec::ContextSpec;
    use andromeda_ngsi_ld_entity::document::{EntityDocument, attribute_member::AttributeMember, instance_document::InstanceDocument};
    use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier};
    use andromeda_ngsi_ld_model::registration::ContextSourceRegistration;
    use indexmap::IndexMap;
    use serde_json::json;

    const SPEED: &str = "https://example.org/speed";

    const ENTITY_ID: &str = "urn:ngsi-ld:Vehicle:1";

    /// An exclusive registration covering the fixture entity that advertises
    /// no write, so the forwarding answers it without contacting it.
    fn exclusive_registration_advertising_no_write() -> ContextSourceRegistration {
        serde_json::from_value(json!({
            "id": "urn:ngsi-ld:csr:exclusive",
            "type": "ContextSourceRegistration",
            "endpoint": "https://peer.example.org",
            "mode": "exclusive",
            "information": [{"entities": [{"id": ENTITY_ID, "type": "https://example.org/Vehicle"}]}],
            "operations": ["retrieveEntity"],
            "status": "ok",
            "createdAt": "2024-01-01T00:00:00Z",
            "modifiedAt": "2024-01-01T00:00:00Z"
        }))
        .expect("valid registration")
    }

    /// The admitted document of a Vehicle carrying one `speed` instance.
    fn vehicle_document() -> EntityDocument {
        let speed: AttributeName = SPEED.parse().expect("an attribute name");
        EntityDocument::new(
            Some(ENTITY_ID.parse().expect("valid identifier")),
            Vec::new(),
            None,
            None,
            IndexMap::from([(speed, AttributeMember::Instances(vec![InstanceDocument::builder().build()]))]),
        )
    }

    /// Forward a create of the fixture entity whose admitted content is
    /// `document`.
    async fn forward_create(service: &FederationService, document: &EntityDocument) -> Vec<ForwardedWriteResult> {
        let entity_id: Identifier = ENTITY_ID.parse().expect("valid identifier");
        let context = ContextSpec::none();
        service
            .forward_provision(
                &ContextTarget::Live,
                ForwardProvisionRequest {
                    resource: ProvisionResource::Entities,
                    entity_id: &entity_id,
                    entity_type_compact: None,
                    attribute_name: None,
                    method: reqwest::Method::POST,
                    context: &context,
                    operation: ForwardingOperation::CreateEntity,
                    path_suffix: None,
                    query_string: None,
                    triggering: TriggeringRequest { via: None, headers: None },
                },
                Some(ForwardedContent::Entity(document)),
            )
            .await
            .expect("the forwarding completes")
    }

    /// Clause 5.12: a write no registration matches is not forwarded.
    #[tokio::test]
    async fn a_write_no_registration_matches_is_not_forwarded() {
        let results = forward_create(&offline_service(Vec::new()), &vehicle_document()).await;
        assert!(results.is_empty(), "{:?}", results.iter().map(|result| &result.outcome).collect::<Vec<_>>());
    }

    /// The attribute segment is the one after the `attrs` collection, so an
    /// attribute named `attrs` is spelled once and the collection stays as it
    /// is.
    #[test]
    fn the_attribute_segment_is_replaced_by_its_position() {
        let spelled: AttributeName = "velocity".parse().expect("an attribute name");
        pretty_assertions::assert_eq!(spelled_suffix(Some("attrs/speed"), Some(&spelled)), vec!["attrs", "velocity"]);
        let spelled_attrs: AttributeName = "https://example.org/attrs".parse().expect("an attribute name");
        pretty_assertions::assert_eq!(
            spelled_suffix(Some("attrs/attrs"), Some(&spelled_attrs)),
            vec!["attrs", "https://example.org/attrs"]
        );
        pretty_assertions::assert_eq!(
            spelled_suffix(Some("attrs/speed/urn:ngsi-ld:instance:1"), Some(&spelled)),
            vec!["attrs", "velocity", "urn:ngsi-ld:instance:1"]
        );
        pretty_assertions::assert_eq!(spelled_suffix(Some("attrs"), Some(&spelled)), vec!["attrs"]);
        pretty_assertions::assert_eq!(spelled_suffix(Some("attrs/speed"), None), vec!["attrs", "speed"]);
        pretty_assertions::assert_eq!(spelled_suffix(None, Some(&spelled)), Vec::<&str>::new());
    }

    /// Clauses 5.6.1.4 and following: a proxied registration that advertises
    /// no write still claims the attributes the content writes, by their
    /// expanded names.
    #[tokio::test]
    async fn a_matched_registration_claims_the_expanded_names_of_the_content() {
        let results = forward_create(&offline_service(vec![exclusive_registration_advertising_no_write()]), &vehicle_document()).await;
        let [result] = results.as_slice() else {
            panic!("exactly the one registration answers: {}", results.len());
        };
        assert!(matches!(result.outcome, ForwardedWriteOutcome::NotAdvertised), "{:?}", result.outcome);
        pretty_assertions::assert_eq!(result.proxied_attrs, vec![SPEED.parse::<AttributeName>().expect("an attribute name")]);
    }
}
