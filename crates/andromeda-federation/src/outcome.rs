//! Typed results of context-source federation attempts.

use crate::{
    answer_body::AnswerBody,
    error::FederationError,
    source_continuation::SourceContinuation,
    source_read_warning::SourceReadWarning,
    temporal_history_walk::least_range_end,
};
use andromeda_entity_map::contribution::SourceContribution;
use andromeda_http_headers::{ngsild_content_range::NgsiLdContentRange, ngsild_warning::NgsiLdWarning};
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_expansion::received_state::ReceivedState;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier};
use andromeda_ngsi_ld_model::{registration::forwarding_mode::ForwardingMode, temporal::temporal_history::TemporalHistory};
use andromeda_outbound_http::endpoint_refusal::EndpointRefusal;
use chrono::{DateTime, Utc};
use std::{sync::Arc, time::Duration};
use thiserror::Error;
use tracing::warn;

/// Why no HTTP response was obtained from a context source.
///
/// Shared between every outcome that reports the attempt, so the causes that
/// are not `Clone` are held behind an `Arc`.
#[derive(Debug, Clone, Error)]
pub enum TransportFailure {
    /// The source did not answer within the registration's timeout.
    #[error("no response within {}ms", .after.as_millis())]
    TimedOut {
        /// The timeout that elapsed.
        after: Duration,
    },
    /// The source is inside its clause 5.2.34 cooldown window and was not
    /// contacted.
    #[error("registration is cooling down for another {}ms after a failure", .remaining.as_millis())]
    CoolingDown {
        /// How much of the window is left.
        remaining: Duration,
    },
    /// The broker could not prepare the request for this registration.
    #[error("the forwarded request could not be prepared: {0}")]
    Unprepared(Arc<FederationError>),
    /// The request could not be delivered: connection refused, DNS failure or
    /// TLS failure.
    #[error("the forwarded request could not be delivered: {0}")]
    Undelivered(Arc<reqwest::Error>),
    /// The endpoint is refused before anything is sent to it: it names no
    /// host, cannot be resolved, or resolves into private or reserved address
    /// space while `federation.forwarding.private_ip_blocking` is `refused`.
    /// The cause is not part of the message, since a warning reaches the client
    /// and the address belongs to the broker's network.
    #[error("the forwarded request was refused before it was sent")]
    Refused(#[source] EndpointRefusal),
    /// The source answered, but names in its answer do not expand against the
    /// context the request was written in, so the answer cannot be read in
    /// Fully Qualified Names (clauses 4.3.6.6 and 6.3.6).
    #[error("the context source's answer could not be read: {0}")]
    Unreadable(Arc<FederationError>),
}

impl TransportFailure {
    /// A failure to prepare the request for a registration.
    #[must_use]
    pub fn unprepared(error: FederationError) -> TransportFailure {
        TransportFailure::Unprepared(Arc::new(error))
    }

    /// Whether the failure is the source not answering in time, which clause
    /// 6.3.17 reports as a gateway timeout rather than a bad gateway.
    #[must_use]
    pub const fn is_timeout(&self) -> bool {
        matches!(self, TransportFailure::TimedOut { .. } | TransportFailure::CoolingDown { .. })
    }
}

/// What a context source answered a successful GET with.
#[derive(Debug, Clone)]
pub(crate) struct ForwardedResponse {
    /// The body, as the source wrote it.
    pub(crate) body: Arc<[u8]>,
    /// The context the source's answer is spelled in: the forwarding context
    /// the request was expressed against (clause 4.3.6.6). Boxed: a context
    /// specification is several times the size of every other member.
    pub(crate) naming: Box<ContextSpec>,
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
            ForwardGetOutcome::Success(_) | ForwardGetOutcome::NotFound(_) | ForwardGetOutcome::Transport(_) | ForwardGetOutcome::LoopDetected => None,
        }
    }

    /// The clause 6.3.17 warning a failed GET contributes, if any: a 404 is not
    /// abnormal and contributes none. A request this broker could not prepare
    /// is reported without its cause, which is logged instead.
    pub(crate) fn warning(&self, registration: &Identifier) -> Option<NgsiLdWarning> {
        let warning = match self {
            ForwardGetOutcome::Success(_) | ForwardGetOutcome::NotFound(_) => return None,
            ForwardGetOutcome::ErrorStatus(status) => SourceReadWarning::ErrorStatus { registration, status: *status },
            ForwardGetOutcome::Transport(TransportFailure::Unprepared(error)) => {
                warn!(%registration, %error, "the request forwarded to a context source could not be prepared");
                SourceReadWarning::Unprepared { registration }
            }
            ForwardGetOutcome::Transport(failure) => SourceReadWarning::Transport { registration, failure },
            ForwardGetOutcome::LoopDetected => SourceReadWarning::LoopDetected { registration },
        };
        Some(warning.into_warning())
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
    /// The entities the context source returned, read into the model under
    /// the context each was written in.
    pub entities: Vec<ReceivedState>,
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
            entity_ids: self.entities.iter().map(|received| received.state.id().clone()).collect(), // clone: same
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
    /// The history of every entity the source answered for, read into the
    /// model under the context it was written in and gathered by entity.
    pub entities: Vec<TemporalHistory>,
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
    /// nothing for the entry that only carries the loop warning.
    #[must_use]
    pub fn contribution(&self) -> Option<SourceContribution> {
        Some(SourceContribution {
            registration: self.registration_id.clone()?, // clone: the contribution owns the registration it names
            entity_ids: self.entities.iter().filter_map(|history| history.id().cloned()).collect(), // clone: the contribution owns the identifiers it names
            linked_map: self.linked_map.clone(),         // clone: same
        })
    }
    /// The earliest instant a page of the answer was cut at (clause 6.3.10),
    /// up to which every instance of what the answer renders is held; `None`
    /// when the answer is complete.
    #[must_use]
    pub fn least_range_end(&self) -> Option<DateTime<Utc>> {
        least_range_end(&self.partial_ranges)
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
}

/// Result of one provision forwarding attempt.
pub struct ForwardedWriteResult {
    /// The registration the operation was forwarded to.
    pub registration_id: Identifier,
    /// Registration forwarding mode.
    pub mode: ForwardingMode,
    /// Typed outbound result.
    pub outcome: ForwardedWriteOutcome,
    /// The attributes of the write the registration covers (clause 4.3.6.1),
    /// by their expanded names: what it was sent, or would have been sent had
    /// it advertised the operation. Empty for a registration the write reached
    /// for its entity alone (a loop or a purge).
    pub attributes: Vec<AttributeName>,
}

/// Result classification for one forwarded write.
#[derive(Debug, Clone)]
pub enum ForwardedWriteOutcome {
    /// The context source completed the operation.
    Completed {
        /// Success status.
        status: u16,
        /// The success response body.
        body: AnswerBody,
    },
    /// The context source returned an HTTP error.
    RemoteHttpFailure {
        /// Error status.
        status: u16,
        /// The problem detail, when the source sent one.
        body: AnswerBody,
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
    /// The attributes the registration takes out of local processing: all it
    /// covers when it is proxied (exclusive or redirect, clause 4.3.6.3), none
    /// when it is inclusive.
    #[must_use]
    pub fn proxied_attrs(&self) -> &[AttributeName] {
        match self.mode {
            ForwardingMode::Exclusive | ForwardingMode::Redirect => &self.attributes,
            ForwardingMode::Inclusive | ForwardingMode::Auxiliary => &[],
        }
    }

    /// HTTP status represented by this result, using the clause 6.3.17 codes
    /// for the failures that produced no response.
    #[must_use]
    pub const fn status(&self) -> u16 {
        match &self.outcome {
            ForwardedWriteOutcome::Completed { status, .. } | ForwardedWriteOutcome::RemoteHttpFailure { status, .. } => *status,
            ForwardedWriteOutcome::Transport(TransportFailure::TimedOut { .. } | TransportFailure::CoolingDown { .. }) => 504,
            ForwardedWriteOutcome::Transport(
                TransportFailure::Unprepared(_) | TransportFailure::Undelivered(_) | TransportFailure::Refused(_) | TransportFailure::Unreadable(_),
            ) => 502,
            ForwardedWriteOutcome::LoopDetected => 508,
            ForwardedWriteOutcome::NotAdvertised => 409,
            ForwardedWriteOutcome::UpsertFollowUpNotAdvertised => 422,
        }
    }

    /// Whether the forwarding attempt failed.
    #[must_use]
    pub const fn is_failure(&self) -> bool {
        !matches!(self.outcome, ForwardedWriteOutcome::Completed { .. })
    }
}

/// Result of one discovery forwarding attempt.
pub struct ForwardedDiscoveryResult {
    /// The registration that answered; `None` for the entry that carries only
    /// the clause 6.3.17 loop warning.
    pub registration_id: Option<Identifier>,
    /// The successful response body, as the source wrote it.
    pub body: Option<Arc<[u8]>>,
    /// The context the body is spelled in.
    pub naming: ContextSpec,
    /// Protocol warnings generated by the attempt.
    pub warnings: Vec<NgsiLdWarning>,
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        outcome::{ForwardGetOutcome, ForwardedResponse, ForwardedWriteOutcome, ForwardedWriteResult, TransportFailure},
        source_continuation::SourceContinuation,
    };
    use andromeda_jsonld::context_spec::ContextSpec;
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use andromeda_ngsi_ld_model::registration::forwarding_mode::ForwardingMode;
    use std::{sync::Arc, time::Duration};

    /// Only an HTTP error status is a status a source answered with. The
    /// outcomes that produced no response, and the ones that produced a good
    /// one, name none, so nothing but a real answer can ever be relayed as the
    /// operation's own (Table 6.4.3.2-3, Table 6.34.3.1-1).
    #[test]
    fn only_an_error_status_answer_names_a_status() {
        let declined = ForwardGetOutcome::ErrorStatus(501);
        pretty_assertions::assert_eq!(declined.error_status(), Some(501));

        let success = ForwardGetOutcome::Success(ForwardedResponse {
            body: Arc::from(b"[]".as_slice()),
            naming: Box::new(ContextSpec::none()),
            warnings: Vec::new(),
            entity_map: None,
            continuation: SourceContinuation::Complete,
            partial_range: None,
        });
        pretty_assertions::assert_eq!(success.error_status(), None);
        pretty_assertions::assert_eq!(ForwardGetOutcome::NotFound(Vec::new()).error_status(), None);
        pretty_assertions::assert_eq!(
            ForwardGetOutcome::Transport(TransportFailure::TimedOut {
                after: Duration::from_millis(100)
            })
            .error_status(),
            None
        );
        pretty_assertions::assert_eq!(ForwardGetOutcome::LoopDetected.error_status(), None);
    }

    /// Clause 4.3.6.3: only an exclusive or redirect registration takes the
    /// attributes it covers out of local processing.
    #[test]
    fn only_a_proxied_registration_takes_its_attributes_out_of_local_processing() {
        let speed = || "https://example.org/speed".parse::<AttributeName>().expect("an attribute name");
        for (mode, proxied) in [
            (ForwardingMode::Exclusive, vec![speed()]),
            (ForwardingMode::Redirect, vec![speed()]),
            (ForwardingMode::Inclusive, Vec::new()),
        ] {
            let result = ForwardedWriteResult {
                registration_id: "urn:ngsi-ld:ContextSourceRegistration:1".parse().expect("valid URI"),
                mode,
                outcome: ForwardedWriteOutcome::NotAdvertised,
                attributes: vec![speed()],
            };
            pretty_assertions::assert_eq!(result.proxied_attrs(), proxied.as_slice(), "{mode:?}");
        }
    }
}
