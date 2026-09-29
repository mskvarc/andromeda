//! Federated single-entity provision operations (clauses 4.3.6, 5.6, 6.3.17),
//! on the current entity or on its temporal evolution.

use crate::{
    FederationService,
    error::FederationError,
    forwarded_body::ForwardedBody,
    forwarding_url::forwarding_url,
    outcome::{ForwardedWriteOutcome, ForwardedWriteResult, TransportFailure},
    registration_selection::{collect_matching_attrs, csr_has_matching_information, filter_fragment_for_registration},
    request::{ForwardProvisionRequest, TriggeringRequest},
    term_resolver::TermResolver,
    transport::ForwardedRequest,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier};
use andromeda_ngsi_ld_model::registration::{ContextSourceRegistration, forwarding_mode::ForwardingMode};
use andromeda_telemetry::federation_ops::{self, FederationOperation};
use futures::{StreamExt, stream};
use iri_rs::IriBuf;
use serde_json::Value;
use tracing::{debug, warn};
use url::Url;

/// One write to forward to one registration, decided before it is sent.
pub(crate) struct PlannedWrite<'a> {
    /// The registration to contact.
    pub(crate) registration: ContextSourceRegistration,
    /// The forwarded URL.
    pub(crate) url: Result<Url, FederationError>,
    /// The HTTP method.
    pub(crate) method: reqwest::Method,
    /// The body narrowed to what the registration covers.
    pub(crate) body: Option<Vec<u8>>,
    /// The attributes an exclusive registration claims from the payload.
    pub(crate) proxied_attrs: Vec<AttributeName>,
    /// The context of the triggering request.
    pub(crate) context: &'a ContextSpec,
    /// The triggering request.
    pub(crate) triggering: TriggeringRequest<'a>,
    /// The `NGSILD-EntityMap` this forwarded write carries: the map the source
    /// is linked to under the Entity Map in use (clause 5.5.14), or `None` when
    /// the write runs under no map.
    pub(crate) linked_map: Option<Identifier>,
}

/// One forwarded write request, as [`FederationService::forward_write`] sends it.
///
/// The fields are what the transport needs to build and classify one request;
/// they travel as a struct rather than a long argument list.
pub(crate) struct ForwardWrite<'a> {
    /// The registration being contacted.
    pub(crate) registration: &'a ContextSourceRegistration,
    /// The forwarded URL, or the failure that stops it being built.
    pub(crate) url: Result<Url, FederationError>,
    /// The HTTP method.
    pub(crate) method: reqwest::Method,
    /// The request body, already narrowed to what the registration covers.
    pub(crate) body: Option<&'a [u8]>,
    /// The context of the triggering request.
    pub(crate) context: &'a ContextSpec,
    /// The request that triggered the forwarding.
    pub(crate) triggering: TriggeringRequest<'a>,
    /// The `NGSILD-EntityMap` the forwarded request carries (clause 5.5.14).
    pub(crate) linked_map: Option<&'a Identifier>,
}

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
                if let Some(write) = plan_write(csr, &request, body.as_deref(), &expanded_types, &resolver).await? {
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

    /// Send every planned write, at most `csr_concurrency` at a time.
    pub(crate) async fn execute_writes(&self, planned: Vec<PlannedWrite<'_>>) -> Vec<ForwardedWriteResult> {
        stream::iter(planned)
            .map(|write| self.execute_write(write))
            .buffer_unordered(self.csr_concurrency)
            .collect()
            .await
    }

    /// Send one planned write and classify what came back.
    pub(crate) async fn execute_write(&self, write: PlannedWrite<'_>) -> ForwardedWriteResult {
        let PlannedWrite {
            registration,
            url,
            method,
            body,
            proxied_attrs,
            context,
            triggering,
            linked_map,
        } = write;
        let mode = registration.access.mode.unwrap_or_default();
        let outcome = self
            .forward_write(ForwardWrite {
                registration: &registration,
                url,
                method,
                body: body.as_deref(),
                context,
                triggering,
                linked_map: linked_map.as_ref(),
            })
            .await;
        if let ForwardedWriteOutcome::Transport(failure) = &outcome {
            warn!("Failed to forward to context source {}: {}", registration.id, failure.detail());
        }
        ForwardedWriteResult {
            registration_id: registration.id,
            mode,
            outcome,
            proxied_attrs,
        }
    }

    /// Make a write request (POST/PUT/PATCH/DELETE) to a context source endpoint.
    pub(crate) async fn forward_write(&self, write: ForwardWrite<'_>) -> ForwardedWriteOutcome {
        let ForwardWrite {
            registration,
            url,
            method,
            body,
            context,
            triggering,
            linked_map,
        } = write;
        if self.registration_loops(registration, triggering.via) {
            return ForwardedWriteOutcome::LoopDetected;
        }
        let unreachable = |detail: String| ForwardedWriteOutcome::Transport(TransportFailure::Unreachable { detail });
        let url = match url {
            Ok(url) => url,
            Err(error) => return unreachable(error.to_string()),
        };
        if let Some(remaining) = self.cooldowns.remaining(registration) {
            // Clause 5.2.34: a request inside the cooldown window is answered
            // with a timeout for the registration, without contacting it.
            return ForwardedWriteOutcome::Transport(TransportFailure::Timeout {
                detail: format!("registration is cooling down for another {}ms after a failure", remaining.as_millis()),
            });
        }
        debug!("CS forwarding {} {}", method, url);
        let payload = match self.payload_for(registration, context, body).await {
            Ok(payload) => payload,
            Err(error) => return unreachable(error.to_string()),
        };
        let request = match self.build_request(ForwardedRequest {
            method,
            url,
            payload: &payload,
            registration,
            triggering,
            linked_map,
        }) {
            Ok(request) => request,
            Err(error) => return unreachable(error.to_string()),
        };

        let sent = match self.send_request(registration, request).await {
            Ok(sent) => sent,
            Err(failure) => return ForwardedWriteOutcome::Transport(failure),
        };

        let response_body = if sent.body.is_empty() {
            None
        } else {
            serde_json::from_slice::<Value>(&sent.body).ok()
        };
        let status = sent.status.as_u16();
        if sent.status.is_success() {
            ForwardedWriteOutcome::Completed { status, body: response_body }
        } else {
            ForwardedWriteOutcome::RemoteHttpFailure { status, body: response_body }
        }
    }
}

/// Plan the one forwarded write to `csr` for `request`, whose payload
/// serialised to `body`, or `None` when the registration covers nothing of the
/// body once it is narrowed to what the registration declares (clause
/// 4.3.6.1).
///
/// # Errors
///
/// Returns [`FederationError::JsonLd`] when a body attribute cannot be expanded
/// against the request context.
async fn plan_write<'a>(
    csr: ContextSourceRegistration,
    request: &ForwardProvisionRequest<'a>,
    body: Option<&[u8]>,
    entity_types: &[IriBuf],
    resolver: &TermResolver<'_>,
) -> Result<Option<PlannedWrite<'a>>, FederationError> {
    // A POST below the bare resource creates (clauses 6.4.3.1 and 6.18.3.1);
    // every other write addresses the entity itself.
    let mut path = request.resource.segments().to_vec();
    if !(request.method == reqwest::Method::POST && request.path_suffix.is_none()) {
        path.push(request.entity_id.as_str());
        path.extend(
            request
                .path_suffix
                .into_iter()
                .flat_map(|suffix| suffix.split('/'))
                .filter(|segment| !segment.is_empty()),
        );
    }
    let url = forwarding_url(csr.access.endpoint.url(), &path, request.query_string.unwrap_or(""));

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
        request::{ForwardProvisionRequest, TriggeringRequest},
    };
    use andromeda_context_space::context_target::ContextTarget;
    use andromeda_jsonld::{context_spec::ContextSpec, engine::processor::JsonLdEngine};
    use andromeda_ngsi_ld_lexicon::{context_source_pseudonym::ContextSourcePseudonym, identifier::Identifier};
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
