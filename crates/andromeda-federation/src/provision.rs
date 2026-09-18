//! Federated single-entity provision operations (clauses 4.3.6, 5.6, 6.3.17),
//! on the current entity or on its temporal evolution.

use crate::{
    FederationService,
    error::FederationError,
    federation_conflict_reason::FederationConflictReason,
    federation_operation_unsupported::FederationOperationUnsupported,
    forwarding_url::forwarding_url,
    outcome::{ForwardedWriteOutcome, ForwardedWriteResult, TransportFailure},
    registration_selection::{collect_matching_attrs, csr_has_matching_information, filter_fragment_for_registration},
    request::{ForwardProvisionRequest, TriggeringRequest},
    transport::ForwardedRequest,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, uri::Uri};
use andromeda_ngsi_ld_model::registration::{ContextSourceRegistration, forwarding_mode::ForwardingMode};
use andromeda_telemetry::federation_ops::{self, FederationOperation};
use futures::{StreamExt, stream};
use iri_rs::IriBuf;
use serde_json::Value;
use std::time::Duration;
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
    pub(crate) linked_map: Option<Uri>,
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
    pub(crate) linked_map: Option<&'a Uri>,
}

impl FederationService {
    /// Forward a single NGSI-LD provision operation.
    ///
    /// # Errors
    ///
    /// Returns [`FederationError::JsonLd`] when the request's entity type
    /// cannot be expanded against its context, and
    /// [`FederationError::Registration`] when the matching registrations
    /// cannot be read. A source that rejects the write does not fail the
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

            let prepared_context = self.engine.prepare_term_context(request.context);
            let expanded_type = if let Some(t) = request.entity_type_compact {
                Some(self.engine.expand_type(t.as_str(), &prepared_context).await?)
            } else {
                None
            };

            let csrs = self.csr_repo.find_matching(expanded_type.as_ref(), Some(request.entity_id)).await?;

            // Per spec 4.3.6.2: auxiliary CSRs only supplement retrieval, never receive writes.
            let mut matching_csrs = Vec::new();
            let mut results = Vec::new();
            for csr in csrs {
                if csr.access.mode == Some(ForwardingMode::Auxiliary) {
                    continue;
                }
                if !csr_has_matching_information(&csr, expanded_type.as_ref(), Some(request.entity_id), request.attribute_name) {
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
                    // Clauses 5.6.1.4 and following: a proxied registration that
                    // does not support the operation still owns its attributes, so
                    // the write fails for them instead of landing here.
                    let proxied_attrs = collect_matching_attrs(&csr, expanded_type.as_ref(), Some(request.entity_id), request.attribute_name, request.body);
                    let mode = proxied_mode(&csr);
                    results.push(ForwardedWriteResult {
                        registration_id: csr.id,
                        mode,
                        outcome: ForwardedWriteOutcome::NotAdvertised,
                        proxied_attrs,
                    });
                }
            }

            let expanded_type_ref = expanded_type.as_ref();
            let planned: Vec<PlannedWrite<'_>> = matching_csrs
                .into_iter()
                .filter_map(|csr| plan_write(csr, &request, expanded_type_ref))
                .collect();

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

/// Plan the one forwarded write to `csr` for `request`, or `None` when the
/// registration covers nothing of the body once it is narrowed to what the
/// registration declares (clause 4.3.6.1).
fn plan_write<'a>(csr: ContextSourceRegistration, request: &ForwardProvisionRequest<'a>, expanded_type: Option<&IriBuf>) -> Option<PlannedWrite<'a>> {
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
        collect_matching_attrs(&csr, expanded_type, Some(request.entity_id), request.attribute_name, request.body)
    } else {
        Vec::new()
    };

    let body = if request.attribute_name.is_none() {
        filter_fragment_for_registration(request.body, &csr, expanded_type, Some(request.entity_id))
    } else {
        request.body.map(<[u8]>::to_vec)
    };
    if request.body.is_some() && body.is_none() {
        return None;
    }
    Some(PlannedWrite {
        registration: csr,
        url,
        method: request.method.clone(), // clone: each planned write owns the method it sends
        body,
        proxied_attrs,
        context: request.context,
        triggering: request.triggering,
        // A single-entity provision runs under no Entity Map.
        linked_map: None,
    })
}

/// Clause 6.3.17: when the data lives in a single exclusive or redirect
/// registration, its failure is the operation's failure and carries the code
/// the clause assigns; when several endpoints were contacted, the results are
/// returned for the 207 Multi-Status path.
/// The clause 6.3.17 error an operation carries when it went entirely to a
/// single proxied (exclusive or redirect) registration and that registration
/// failed it: 508, 504, 404 or 502, or Conflict when the registration does not
/// advertise the operation (clauses 5.6.1.4 and following). `None` when the
/// operation was spread over several sources or partly handled locally, which
/// the caller reports as a 207 instead, and when the single source succeeded.
#[must_use]
pub fn single_source_failure(results: &[ForwardedWriteResult]) -> Option<FederationError> {
    let [single] = results else {
        return None;
    };
    if !matches!(single.mode, ForwardingMode::Exclusive | ForwardingMode::Redirect) {
        return None;
    }
    let registration = single.registration_id.clone(); // clone: the error owns the identifier of the result it describes
    match &single.outcome {
        ForwardedWriteOutcome::Completed { .. } => None,
        ForwardedWriteOutcome::NotAdvertised => Some(FederationError::Conflict(FederationConflictReason::NotAdvertised { registration })),
        ForwardedWriteOutcome::UpsertFollowUpNotAdvertised => Some(FederationError::OperationNotSupported(
            FederationOperationUnsupported::UpsertFollowUpNotAdvertised { registration },
        )),
        ForwardedWriteOutcome::LoopDetected => Some(FederationError::LoopDetected { registration }),
        ForwardedWriteOutcome::Transport(TransportFailure::Timeout { detail }) => Some(FederationError::GatewayTimeout {
            registration,
            timeout: parse_timeout_detail(detail),
        }),
        ForwardedWriteOutcome::Transport(TransportFailure::Unreachable { detail }) => Some(FederationError::BadGateway {
            registration,
            status: None,
            detail: detail.clone(), // clone: same
        }),
        ForwardedWriteOutcome::RemoteHttpFailure { status: 404, .. } => Some(FederationError::RemoteNotFound { registration }),
        // Clause 6.3.17 asks for the most informative error response; a
        // source that refused the operation itself (409, 422) is reported as
        // that refusal rather than folded into 502 Bad Gateway.
        ForwardedWriteOutcome::RemoteHttpFailure { status: status @ 409, body } => Some(FederationError::Conflict(FederationConflictReason::RemoteRejected {
            detail: remote_detail(*status, body.as_ref()),
        })),
        ForwardedWriteOutcome::RemoteHttpFailure { status: status @ 422, body } => {
            Some(FederationError::OperationNotSupported(FederationOperationUnsupported::RemoteUnsupported {
                detail: remote_detail(*status, body.as_ref()),
            }))
        }
        ForwardedWriteOutcome::RemoteHttpFailure { status, body } => Some(FederationError::BadGateway {
            registration,
            status: Some(*status),
            detail: remote_detail(*status, body.as_ref()),
        }),
    }
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

/// The most informative detail a remote error carries: its problem `detail`,
/// its `title`, or the bare status.
fn remote_detail(status: u16, body: Option<&Value>) -> String {
    body.and_then(|body| body.get("detail").or_else(|| body.get("title")))
        .and_then(Value::as_str)
        .map_or_else(
            || format!("context source answered HTTP {status}"),
            |detail| format!("context source answered HTTP {status}: {detail}"),
        )
}

/// Recover the deadline a timeout detail names, for the typed error.
fn parse_timeout_detail(detail: &str) -> Duration {
    detail
        .split(|character: char| !character.is_ascii_digit())
        .filter(|token| !token.is_empty())
        .find_map(|token| token.parse::<u64>().ok())
        .map_or(Duration::ZERO, Duration::from_millis)
}

#[cfg(test)]
mod tests {
    use crate::{
        error::FederationError,
        outcome::{ForwardedWriteOutcome, ForwardedWriteResult, TransportFailure},
        provision::{remote_detail, single_source_failure},
    };
    use andromeda_ngsi_ld_model::registration::forwarding_mode::ForwardingMode;
    use serde_json::json;

    fn result(mode: ForwardingMode, outcome: ForwardedWriteOutcome) -> ForwardedWriteResult {
        ForwardedWriteResult {
            registration_id: "urn:ngsi-ld:csr:1".parse().expect("valid URI"),
            mode,
            outcome,
            proxied_attrs: Vec::new(),
        }
    }

    #[test]
    fn a_single_proxied_source_failure_becomes_the_clause_6_3_17_error() {
        let cases = [
            (ForwardedWriteOutcome::LoopDetected, "loop"),
            (
                ForwardedWriteOutcome::Transport(TransportFailure::Timeout {
                    detail: "no response within 5000ms".to_string(),
                }),
                "timeout",
            ),
            (
                ForwardedWriteOutcome::Transport(TransportFailure::Unreachable {
                    detail: "connection refused".to_string(),
                }),
                "unreachable",
            ),
            (ForwardedWriteOutcome::RemoteHttpFailure { status: 404, body: None }, "not found"),
            (ForwardedWriteOutcome::NotAdvertised, "not advertised"),
            (ForwardedWriteOutcome::UpsertFollowUpNotAdvertised, "no follow-up"),
            (
                ForwardedWriteOutcome::RemoteHttpFailure {
                    status: 409,
                    body: Some(json!({"detail": "exists"})),
                },
                "remote conflict",
            ),
            (ForwardedWriteOutcome::RemoteHttpFailure { status: 422, body: None }, "remote unsupported"),
            (
                ForwardedWriteOutcome::RemoteHttpFailure {
                    status: 403,
                    body: Some(json!({"detail": "no access"})),
                },
                "forbidden",
            ),
        ];
        for mode in [ForwardingMode::Exclusive, ForwardingMode::Redirect] {
            for (outcome, label) in cases.clone() {
                let escalated = single_source_failure(&[result(mode, outcome)]);
                match label {
                    "loop" => assert!(matches!(escalated, Some(FederationError::LoopDetected { .. })), "{mode:?} {label}"),
                    "timeout" => assert!(
                        matches!(escalated, Some(FederationError::GatewayTimeout { timeout, .. }) if timeout.as_millis() == 5000),
                        "{mode:?} {label}"
                    ),
                    "unreachable" => assert!(matches!(escalated, Some(FederationError::BadGateway { status: None, .. })), "{mode:?} {label}"),
                    "not found" => assert!(matches!(escalated, Some(FederationError::RemoteNotFound { .. })), "{mode:?} {label}"),
                    "not advertised" => assert!(matches!(escalated, Some(FederationError::Conflict(_))), "{mode:?} {label}"),
                    "no follow-up" | "remote unsupported" => {
                        assert!(matches!(escalated, Some(FederationError::OperationNotSupported(_))), "{mode:?} {label}");
                    }
                    "remote conflict" => assert!(
                        matches!(&escalated, Some(error @ FederationError::Conflict(_)) if error.to_string().contains("exists")),
                        "{mode:?} {label}"
                    ),
                    other => assert!(
                        matches!(escalated, Some(FederationError::BadGateway { status: Some(403), ref detail, .. }) if detail.contains("no access")),
                        "{mode:?} {other}"
                    ),
                }
            }
        }
    }

    #[test]
    fn a_completed_single_source_and_an_inclusive_failure_pass_through() {
        let completed = single_source_failure(&[result(ForwardingMode::Exclusive, ForwardedWriteOutcome::Completed { status: 204, body: None })]);
        assert!(completed.is_none());
        let inclusive = single_source_failure(&[result(
            ForwardingMode::Inclusive,
            ForwardedWriteOutcome::RemoteHttpFailure { status: 500, body: None },
        )]);
        assert!(inclusive.is_none());
    }

    #[test]
    fn several_sources_stay_results_for_the_207_path() {
        let results = vec![
            result(ForwardingMode::Redirect, ForwardedWriteOutcome::RemoteHttpFailure { status: 500, body: None }),
            result(ForwardingMode::Redirect, ForwardedWriteOutcome::Completed { status: 204, body: None }),
        ];
        assert!(single_source_failure(&results).is_none());
    }

    #[test]
    fn the_remote_detail_prefers_the_problem_detail_then_the_title_then_the_status() {
        pretty_assertions::assert_eq!(
            remote_detail(403, Some(&json!({"detail": "denied"}))),
            "context source answered HTTP 403: denied"
        );
        pretty_assertions::assert_eq!(
            remote_detail(403, Some(&json!({"title": "Forbidden"}))),
            "context source answered HTTP 403: Forbidden"
        );
        pretty_assertions::assert_eq!(remote_detail(500, None), "context source answered HTTP 500");
    }
}
