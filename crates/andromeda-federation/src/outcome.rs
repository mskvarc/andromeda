//! Typed results of context-source federation attempts.

use crate::source_continuation::SourceContinuation;
use andromeda_entity_map::contribution::SourceContribution;
use andromeda_http_headers::{ngsild_content_range::NgsiLdContentRange, ngsild_warning::NgsiLdWarning};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier};
use andromeda_ngsi_ld_model::{entity::Entity, registration::forwarding_mode::ForwardingMode};
use serde_json::Value;

/// Why no HTTP response was obtained from a context source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransportFailure {
    /// The source did not answer within the registration's timeout, or is
    /// inside its cooldown window and was not contacted (clause 5.2.34).
    Timeout {
        /// Sanitized transport detail, relayed opaquely from the failed attempt
        /// and not broker-authored.
        detail: String,
    },
    /// The request could not be delivered: connection refused, DNS failure,
    /// TLS failure, or a malformed request.
    Unreachable {
        /// Sanitized transport detail, relayed opaquely from the failed attempt
        /// and not broker-authored.
        detail: String,
    },
}

impl TransportFailure {
    /// The sanitized detail of the failure.
    #[must_use]
    pub fn detail(&self) -> &str {
        match self {
            TransportFailure::Timeout { detail } | TransportFailure::Unreachable { detail } => detail,
        }
    }
}

/// What a context source answered a successful GET with.
#[derive(Debug, Clone)]
pub(crate) struct ForwardedResponse {
    /// The parsed body.
    pub(crate) body: Value,
    /// The protocol warnings the attempt carries: the source's own clause
    /// 6.3.17 warnings and, for a cached reuse, the 110 warning.
    pub(crate) warnings: Vec<NgsiLdWarning>,
    /// The `EntityMap` the source used for the request, from its
    /// `NGSILD-EntityMap` response header (clause 5.5.14), when it returned one.
    pub(crate) entity_map: Option<Identifier>,
    /// Where the answer continues, from the source's `Link rel="next"`
    /// (clauses 5.5.9.1 and 6.3.10).
    pub(crate) continuation: SourceContinuation,
    /// The temporal `Content-Range` of a 206 answer (clause 6.3.10): how far
    /// into each entity's history the answer reaches.
    pub(crate) partial_range: Option<NgsiLdContentRange>,
}

/// Outcome of a GET request to a context source.
pub(crate) enum ForwardGetOutcome {
    /// Successful JSON response.
    Success(ForwardedResponse),
    /// HTTP 404 response, with the clause 6.3.17 warnings the source reported
    /// about its own sources: a broker that found nothing because its only
    /// registration looped back still names the loop.
    NotFound(Vec<NgsiLdWarning>),
    /// Successful HTTP response with invalid JSON.
    InvalidPayload(String),
    /// Non-success HTTP status other than 404.
    ErrorStatus(u16),
    /// No HTTP response arrived.
    Transport(TransportFailure),
    /// The registration's alias is already in the `Via` chain (clause 4.3.6.4).
    LoopDetected,
}

impl ForwardGetOutcome {
    /// The HTTP error status the source answered with, when it answered with
    /// one at all.
    ///
    /// Table 6.4.3.2-3 and Table 6.34.3.1-1 let a registered source's own
    /// status stand for the operation in the one case where that source holds
    /// all of the data, which is decided from the whole fan-out rather than
    /// from any single answer.
    pub(crate) const fn error_status(&self) -> Option<u16> {
        match self {
            ForwardGetOutcome::ErrorStatus(status) => Some(*status),
            ForwardGetOutcome::Success(_)
            | ForwardGetOutcome::NotFound(_)
            | ForwardGetOutcome::InvalidPayload(_)
            | ForwardGetOutcome::Transport(_)
            | ForwardGetOutcome::LoopDetected => None,
        }
    }

    /// The clause 6.3.17 warning a failed GET contributes, if any: a 404 is not
    /// abnormal and contributes none.
    pub(crate) fn warning(&self, registration: &Identifier) -> Option<NgsiLdWarning> {
        match self {
            ForwardGetOutcome::Success(_) | ForwardGetOutcome::NotFound(_) => None,
            ForwardGetOutcome::InvalidPayload(detail) => Some(NgsiLdWarning::RevalidationFailed(format!(
                "Context source {registration} returned invalid payload: {detail}"
            ))),
            ForwardGetOutcome::ErrorStatus(status) => Some(NgsiLdWarning::MiscellaneousPersistentWarning(format!(
                "Context source {registration} returned HTTP {status}"
            ))),
            ForwardGetOutcome::Transport(failure) => Some(NgsiLdWarning::MiscellaneousWarning(format!(
                "Context source {registration} did not respond: {}",
                failure.detail()
            ))),
            ForwardGetOutcome::LoopDetected => Some(NgsiLdWarning::MiscellaneousWarning(format!("registration loop detected for {registration}"))),
        }
    }
}

/// Forwarded current entities from one registration.
pub struct ForwardedEntities {
    /// The registration the entities came from; `None` for the entry that
    /// carries only the clause 6.3.17 loop warning of a request that already
    /// names this broker in its `Via` chain.
    pub registration_id: Option<Identifier>,
    /// Registration forwarding mode.
    pub mode: ForwardingMode,
    /// Typed entities returned by the context source.
    pub entities: Vec<Entity>,
    /// Protocol warnings generated by the attempt.
    pub warnings: Vec<NgsiLdWarning>,
    /// The `EntityMap` the source used for the request, when it returned one
    /// (clause 5.5.14 `linkedMaps`).
    pub linked_map: Option<Identifier>,
}

impl ForwardedEntities {
    /// What this source contributed to the map of the read (clause 5.2.39):
    /// nothing for the entry that only carries the loop warning.
    #[must_use]
    pub fn contribution(&self) -> Option<SourceContribution> {
        Some(SourceContribution {
            registration: self.registration_id.clone()?, // clone: the contribution owns the registration it names
            entity_ids: self.entities.iter().map(|entity| entity.id.clone()).collect(), // clone: same
            linked_map: self.linked_map.clone(),         // clone: same
        })
    }
}

/// Forwarded temporal entities from one registration.
pub struct ForwardedTemporalEntities {
    /// The registration the entities came from; `None` for the entry that
    /// carries only the clause 6.3.17 loop warning of a request that already
    /// names this broker in its `Via` chain.
    pub registration_id: Option<Identifier>,
    /// Registration forwarding mode.
    pub mode: ForwardingMode,
    /// Temporal entity payloads.
    pub entities: Vec<Value>,
    /// Protocol warnings generated by the attempt.
    pub warnings: Vec<NgsiLdWarning>,
    /// The `EntityMap` the source used for the request, when it returned one
    /// (clause 5.5.14 `linkedMaps`).
    pub linked_map: Option<Identifier>,
    /// The `Content-Range` of every page of the answer that the source cut
    /// short (clause 6.3.10): the instances past each range are still to come
    /// from the source, so the merged history is complete only up to them.
    pub partial_ranges: Vec<NgsiLdContentRange>,
}

impl ForwardedTemporalEntities {
    /// What this source contributed to the map of the read (clause 5.2.39):
    /// nothing for the entry that only carries the loop warning. A payload
    /// without a readable `id` is not an entity and names none.
    #[must_use]
    pub fn contribution(&self) -> Option<SourceContribution> {
        Some(SourceContribution {
            registration: self.registration_id.clone()?, // clone: the contribution owns the registration it names
            entity_ids: self
                .entities
                .iter()
                .filter_map(|value| value.get("id").and_then(Value::as_str).and_then(|raw| raw.parse::<Identifier>().ok()))
                .collect(),
            linked_map: self.linked_map.clone(), // clone: same
        })
    }
}

/// What a forwarded read learnt about the registrations it selected, beyond
/// the entities they returned.
///
/// Table 6.4.3.2-3 lets a source's 501 stand for the operation when that source
/// alone holds the data, which is a fact about the selection rather than about
/// any one answer.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MatchedSources {
    /// How many registrations matched and were (or would have been) contacted.
    pub registrations: usize,
    /// The error status of the one exclusive or redirect registration that
    /// holds all the data, when it declined the request in a way Table
    /// 6.4.3.2-3 lets the broker relay: a 501 for an unsupported representation.
    pub single_source_status: Option<u16>,
}

/// The result of forwarding a read: what each registration answered, and what
/// the selection of registrations itself says.
#[derive(Debug)]
pub struct ForwardReadOutcome<T> {
    /// One entry per contacted registration, plus the entry carrying the loop
    /// warning of a request that already named this broker in its `Via` chain.
    pub results: Vec<T>,
    /// What the registration selection says.
    pub sources: MatchedSources,
}

impl<T> ForwardReadOutcome<T> {
    /// The outcome of a read that selected no registration at all.
    #[must_use]
    pub const fn empty() -> ForwardReadOutcome<T> {
        ForwardReadOutcome {
            results: Vec::new(),
            sources: MatchedSources {
                registrations: 0,
                single_source_status: None,
            },
        }
    }

    /// Whether no registration matched, so clause 5.7.1.4's `ResourceNotFound`
    /// applies when nothing is held locally either.
    #[must_use]
    pub const fn matched_nothing(&self) -> bool {
        self.sources.registrations == 0
    }
}

/// Result of one provision forwarding attempt.
pub struct ForwardedWriteResult {
    /// The registration the operation was forwarded to.
    pub registration_id: Identifier,
    /// Registration forwarding mode.
    pub mode: ForwardingMode,
    /// Typed outbound result.
    pub outcome: ForwardedWriteOutcome,
    /// The attributes a proxied (exclusive or redirect) registration takes out
    /// of local processing (clause 4.3.6.3); empty for an inclusive one.
    pub proxied_attrs: Vec<AttributeName>,
}

/// Result classification for one forwarded write.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ForwardedWriteOutcome {
    /// The context source completed the operation.
    Completed {
        /// Success status.
        status: u16,
        /// Optional success response body.
        body: Option<Value>,
    },
    /// The context source returned an HTTP error.
    RemoteHttpFailure {
        /// Error status.
        status: u16,
        /// Optional problem detail.
        body: Option<Value>,
    },
    /// No HTTP response arrived.
    Transport(TransportFailure),
    /// The registration loops back into the request's own `Via` chain, so it
    /// was not contacted (clauses 4.3.6.4 and 6.3.17).
    LoopDetected,
    /// The proxied registration covers the data but does not advertise the
    /// operation, so it was not contacted; the data it covers is still never
    /// processed locally (clauses 5.6.1.4 and following: an error of type
    /// Conflict when the complete operation fails, partial success otherwise).
    NotAdvertised,
    /// A batch upsert fell back to a per-entity create, the source answered
    /// that the entity already exists, and the registration advertises neither
    /// the replace nor the update the upsert mode calls for (clause 5.6.8.4:
    /// an error of type `OperationNotSupported` for the entity).
    UpsertFollowUpNotAdvertised,
}

impl ForwardedWriteResult {
    /// HTTP status represented by this result, using the clause 6.3.17 codes
    /// for the failures that produced no response.
    #[must_use]
    pub const fn status(&self) -> u16 {
        match &self.outcome {
            ForwardedWriteOutcome::Completed { status, .. } | ForwardedWriteOutcome::RemoteHttpFailure { status, .. } => *status,
            ForwardedWriteOutcome::Transport(TransportFailure::Timeout { .. }) => 504,
            ForwardedWriteOutcome::Transport(TransportFailure::Unreachable { .. }) => 502,
            ForwardedWriteOutcome::LoopDetected => 508,
            ForwardedWriteOutcome::NotAdvertised => 409,
            ForwardedWriteOutcome::UpsertFollowUpNotAdvertised => 422,
        }
    }

    /// Optional response body returned by the context source.
    #[must_use]
    pub const fn body(&self) -> Option<&Value> {
        match &self.outcome {
            ForwardedWriteOutcome::Completed { body, .. } | ForwardedWriteOutcome::RemoteHttpFailure { body, .. } => body.as_ref(),
            ForwardedWriteOutcome::Transport(_)
            | ForwardedWriteOutcome::LoopDetected
            | ForwardedWriteOutcome::NotAdvertised
            | ForwardedWriteOutcome::UpsertFollowUpNotAdvertised => None,
        }
    }

    /// Whether the forwarding attempt failed.
    #[must_use]
    pub const fn is_failure(&self) -> bool {
        !matches!(self.outcome, ForwardedWriteOutcome::Completed { .. })
    }

    /// The informative detail of a failure that produced no response body.
    #[must_use]
    pub fn failure_detail(&self) -> Option<String> {
        match &self.outcome {
            ForwardedWriteOutcome::Transport(failure) => Some(failure.detail().to_string()),
            ForwardedWriteOutcome::LoopDetected => Some(format!("registration {} loops back into the request's Via chain", self.registration_id)),
            ForwardedWriteOutcome::NotAdvertised => Some(format!("registration {} does not advertise the requested operation", self.registration_id)),
            ForwardedWriteOutcome::UpsertFollowUpNotAdvertised => Some(format!(
                "registration {} reported the entity exists and advertises no operation to update it",
                self.registration_id
            )),
            ForwardedWriteOutcome::Completed { .. } | ForwardedWriteOutcome::RemoteHttpFailure { .. } => None,
        }
    }
}

/// Result of one discovery forwarding attempt.
pub struct ForwardedDiscoveryResult {
    /// Successful response body.
    pub body: Option<Value>,
    /// Protocol warnings generated by the attempt.
    pub warnings: Vec<NgsiLdWarning>,
}

#[cfg(test)]
mod tests {
    use crate::{
        outcome::{ForwardGetOutcome, ForwardedResponse, TransportFailure},
        source_continuation::SourceContinuation,
    };
    use serde_json::json;

    /// Only an HTTP error status is a status a source answered with. The
    /// outcomes that produced no response, and the ones that produced a good
    /// one, name none, so nothing but a real answer can ever be relayed as the
    /// operation's own (Table 6.4.3.2-3, Table 6.34.3.1-1).
    #[test]
    fn only_an_error_status_answer_names_a_status() {
        let declined = ForwardGetOutcome::ErrorStatus(501);
        pretty_assertions::assert_eq!(declined.error_status(), Some(501));

        let success = ForwardGetOutcome::Success(ForwardedResponse {
            body: json!([]),
            warnings: Vec::new(),
            entity_map: None,
            continuation: SourceContinuation::Complete,
            partial_range: None,
        });
        pretty_assertions::assert_eq!(success.error_status(), None);
        pretty_assertions::assert_eq!(ForwardGetOutcome::NotFound(Vec::new()).error_status(), None);
        pretty_assertions::assert_eq!(ForwardGetOutcome::InvalidPayload("not JSON".to_owned()).error_status(), None);
        pretty_assertions::assert_eq!(
            ForwardGetOutcome::Transport(TransportFailure::Timeout {
                detail: "no response within 100ms".to_owned()
            })
            .error_status(),
            None
        );
        pretty_assertions::assert_eq!(ForwardGetOutcome::LoopDetected.error_status(), None);
    }
}
