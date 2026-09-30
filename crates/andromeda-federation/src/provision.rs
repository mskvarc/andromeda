//! Federated single-entity provision operations (clauses 4.3.6, 5.6, 6.3.17),
//! on the current entity or on its temporal evolution.

use crate::{
    FederationService,
    error::FederationError,
    forwarded_body::ForwardedBody,
    forwarded_names::ForwardedNames,
    forwarded_write::PlannedWrite,
    forwarding_context::BodyPresence,
    forwarding_url::forwarding_url,
    outcome::{ForwardedWriteOutcome, ForwardedWriteResult},
    registration_payload::forwarding_decision,
    registration_selection::{collect_matching_attrs, csr_has_matching_information, filter_fragment_for_registration},
    request::ForwardProvisionRequest,
    term_resolver::TermResolver,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_model::registration::{ContextSourceRegistration, forwarding_mode::ForwardingMode};
use andromeda_telemetry::federation_ops::{self, FederationOperation};
use iri_rs::IriBuf;
use url::Url;

impl FederationService {
    /// Forward a single NGSI-LD provision operation.
    ///
    /// # Errors
    ///
    /// Returns [`FederationError::JsonLd`] when the request's entity type
    /// cannot be expanded against its context,
    /// [`FederationError::Registration`] when the matching registrations
    /// cannot be read, and [`FederationError::ForwardedBody`] when a
    /// registration matched and the payload cannot be serialised for it. A
    /// source that rejects the write does not fail the
    /// call: its rejection is reported as one of the returned
    /// [`ForwardedWriteResult`]s.
    pub async fn forward_provision(&self, target: &ContextTarget, request: ForwardProvisionRequest<'_>) -> Result<Vec<ForwardedWriteResult>, FederationError> {
        let timer = federation_ops::start(FederationOperation::Provision);
        let result = async move {
            if !target.permits_federation() {
                return Ok(Vec::new());
            }
            // Clause 6.3.18: an incoming request whose Via already names this
            // broker's own hostAlias has looped back onto it. The forward is not
            // sent; how the operation answers depends on the matched registration
            // (clause 6.3.17), decided per registration below.
            let looped = self.is_loop_detected(request.triggering.via);

            let resolver = TermResolver::new(&self.engine, request.context);
            let expanded_types: Vec<IriBuf> = match request.entity_type_compact {
                Some(t) => vec![resolver.expand_type(t.as_str()).await?],
                // An attribute-level write (delete/append/update attrs, merge,
                // replace, delete entity) carries no `type` in its request. Match
                // it against registrations by the local entity's actual types --
                // every one it carries, since clause 5.12 matches on any of them --
                // so a registration scoped to an unrelated type is not taken to
                // cover the write and refuse it. A write to an entity this broker
                // holds only at a source resolves to an empty set and falls back
                // to id-only matching, as before.
                None => self.local_entity_types.local_entity_types(target, request.entity_id).await,
            };
            // The path attribute is a compact term; matching compares IRIs, so
            // it is expanded once against the triggering request's context.
            let expanded_attribute = match request.attribute_name {
                Some(attribute) => Some(resolver.expand_attr(attribute.as_str()).await?),
                None => None,
            };

            // The store pre-filter is a coarse superset; clause-5.12 precision is
            // applied by csr_has_matching_information below. A single known type
            // narrows the query; zero or several types take the whole id-covering
            // superset and let the precise match do the filtering.
            let coarse_type = match expanded_types.as_slice() {
                [single] => Some(single),
                _ => None,
            };
            let csrs = self.csr_repo.find_matching(coarse_type, Some(request.entity_id)).await?;

            // Per spec 4.3.6.2: auxiliary CSRs only supplement retrieval, never receive writes.
            let mut matching_csrs = Vec::new();
            let mut not_advertising = Vec::new();
            let mut results = Vec::new();
            for csr in csrs {
                if csr.access.mode == Some(ForwardingMode::Auxiliary) {
                    continue;
                }
                if !csr_has_matching_information(&csr, &expanded_types, Some(request.entity_id), expanded_attribute.as_ref()) {
                    continue;
                }
                if looped {
                    // Clause 6.3.17: with the request looped back, a proxied source
                    // that holds all the data externally cannot be forwarded to, so
                    // a lone one is 508 Loop Detected (`single_source_failure`
                    // escalates it). An inclusive registration holds local data, so
                    // the loop only stops the forward and the local part still runs.
                    if is_proxied(&csr) {
                        let mode = proxied_mode(&csr);
                        results.push(ForwardedWriteResult {
                            registration_id: csr.id,
                            mode,
                            outcome: ForwardedWriteOutcome::LoopDetected,
                            proxied_attrs: Vec::new(),
                        });
                    }
                    continue;
                }
                if request.operation.matches(&csr) {
                    matching_csrs.push(csr);
                } else if is_proxied(&csr) {
                    not_advertising.push(csr);
                }
            }
            if matching_csrs.is_empty() && not_advertising.is_empty() {
                return Ok(results);
            }

            // A registration matched, so the payload is serialised now and
            // once, for every registration that reads it.
            let body = request
                .body
                .map(ForwardedBody::to_json_bytes)
                .transpose()
                .map_err(FederationError::ForwardedBody)?;
            for csr in not_advertising {
                // Clauses 5.6.1.4 and following: a proxied registration that
                // does not support the operation still owns its attributes, so
                // the write fails for them instead of landing here.
                let proxied_attrs = collect_matching_attrs(
                    &csr,
                    &expanded_types,
                    Some(request.entity_id),
                    request.attribute_name,
                    body.as_deref(),
                    &resolver,
                )
                .await?;
                let mode = proxied_mode(&csr);
                results.push(ForwardedWriteResult {
                    registration_id: csr.id,
                    mode,
                    outcome: ForwardedWriteOutcome::NotAdvertised,
                    proxied_attrs,
                });
            }

            let mut planned: Vec<PlannedWrite<'_>> = Vec::new();
            for csr in matching_csrs {
                if let Some(write) = self.plan_write(csr, &request, body.as_deref(), &expanded_types, &resolver).await? {
                    planned.push(write);
                }
            }

            results.extend(self.execute_writes(planned).await);
            Ok(results)
        }
        .await;
        timer.finish_result(&result);
        result
    }

    /// Plan the one forwarded write to `csr` for `request`, whose payload
    /// serialised to `body`, or `None` when the registration covers nothing of
    /// the body once it is narrowed to what the registration declares (clause
    /// 4.3.6.1).
    ///
    /// # Errors
    ///
    /// Returns [`FederationError::JsonLd`] when a body attribute cannot be
    /// expanded against the request context.
    async fn plan_write<'a>(
        &self,
        csr: ContextSourceRegistration,
        request: &ForwardProvisionRequest<'a>,
        body: Option<&[u8]>,
        entity_types: &[IriBuf],
        resolver: &TermResolver<'_>,
    ) -> Result<Option<PlannedWrite<'a>>, FederationError> {
        let url = self.write_url(&csr, request, body, resolver).await;

        let proxied_attrs = if is_proxied(&csr) {
            collect_matching_attrs(&csr, entity_types, Some(request.entity_id), request.attribute_name, body, resolver).await?
        } else {
            Vec::new()
        };

        let narrowed = if request.attribute_name.is_none() {
            filter_fragment_for_registration(body, &csr, entity_types, Some(request.entity_id), resolver).await?
        } else {
            body.map(<[u8]>::to_vec)
        };
        if body.is_some() && narrowed.is_none() {
            return Ok(None);
        }
        Ok(Some(PlannedWrite {
            registration: csr,
            url,
            method: request.method.clone(), // clone: each planned write owns the method it sends
            body: narrowed,
            proxied_attrs,
            context: request.context,
            triggering: request.triggering,
            // A single-entity provision runs under no Entity Map.
            linked_map: None,
        }))
    }

    /// The URL one forwarded write to `csr` is sent to, with the attribute its
    /// path names and every name-bearing query parameter spelled in the
    /// registration's forwarding context (clause 4.3.6.6), or the failure to
    /// prepare it, which the write then reports as its own.
    async fn write_url(
        &self,
        csr: &ContextSourceRegistration,
        request: &ForwardProvisionRequest<'_>,
        body: Option<&[u8]>,
        resolver: &TermResolver<'_>,
    ) -> Result<Url, FederationError> {
        let presence = match body {
            Some(_) => BodyPresence::Present,
            None => BodyPresence::Absent,
        };
        let decision = forwarding_decision(csr, request.context, presence)?;
        let naming = decision.context.naming(request.context);
        let names = ForwardedNames::new(&self.engine, resolver, &naming, decision.context.rewording());
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
        config::FederationConfig,
        counted_body::CountedBody,
        fixed_registration_store::{FixedRegistrationStore, NoLocalEntities},
        operation::{ForwardingOperation, ProvisionResource},
        outcome::ForwardedWriteOutcome,
        provision::spelled_suffix,
        request::{ForwardProvisionRequest, TriggeringRequest},
    };
    use andromeda_context_space::context_target::ContextTarget;
    use andromeda_jsonld::{context_spec::ContextSpec, engine::processor::JsonLdEngine};
    use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, context_source_pseudonym::ContextSourcePseudonym, identifier::Identifier};
    use andromeda_ngsi_ld_model::registration::ContextSourceRegistration;
    use serde_json::json;
    use std::sync::Arc;

    const ENTITY_ID: &str = "urn:ngsi-ld:Vehicle:1";

    /// A forwarding service choosing among `registrations`, offline.
    fn service(registrations: Vec<ContextSourceRegistration>) -> FederationService {
        FederationService::new(
            Arc::new(FixedRegistrationStore::new(registrations)),
            Arc::new(NoLocalEntities),
            JsonLdEngine::builder().allow_remote(false).build().expect("engine builds"),
            ContextSourcePseudonym::new("andromeda-test").expect("a valid alias"),
            &FederationConfig::default(),
        )
        .expect("the service builds")
    }

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

    /// Forward a create of the fixture entity whose payload is `body`.
    async fn forward_create(service: &FederationService, body: &CountedBody) -> Vec<ForwardedWriteOutcome> {
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
                    body: Some(body),
                    context: &context,
                    operation: ForwardingOperation::CreateEntity,
                    path_suffix: None,
                    query_string: None,
                    triggering: TriggeringRequest { via: None, headers: None },
                },
            )
            .await
            .expect("the forwarding completes")
            .into_iter()
            .map(|result| result.outcome)
            .collect()
    }

    /// Clause 5.12: a write no registration matches is not forwarded, and its
    /// payload is never serialised.
    #[tokio::test]
    async fn a_write_no_registration_matches_never_serialises_its_payload() {
        let body = CountedBody::new(json!({"id": ENTITY_ID, "type": "Vehicle", "speed": {"type": "Property", "value": 1}}));
        let outcomes = forward_create(&service(Vec::new()), &body).await;
        assert!(outcomes.is_empty(), "{outcomes:?}");
        pretty_assertions::assert_eq!(body.serialisations(), 0);
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

    /// Clauses 5.6.1.4 and following: once a registration matches, the
    /// payload is serialised once to decide what the registration claims.
    #[tokio::test]
    async fn a_matched_registration_serialises_the_payload_once() {
        let body = CountedBody::new(json!({"id": ENTITY_ID, "type": "Vehicle", "speed": {"type": "Property", "value": 1}}));
        let outcomes = forward_create(&service(vec![exclusive_registration_advertising_no_write()]), &body).await;
        assert!(matches!(outcomes.as_slice(), [ForwardedWriteOutcome::NotAdvertised]), "{outcomes:?}");
        pretty_assertions::assert_eq!(body.serialisations(), 1);
    }
}
