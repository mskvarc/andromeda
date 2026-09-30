//! Reading what a context source answered a forwarded write with (clauses
//! 4.3.6.6, 5.2.18, 6.3.6 and 6.3.17).
//!
//! The status is the source's verdict on the operation. The body, when there
//! is one, is either the clause 5.2.18 `UpdateResult` of a 207 or a problem
//! detail; a source writes the attribute names of an `UpdateResult` in the
//! context the forwarded request named, while clause 6.3.6 has a 207 body
//! carry Fully Qualified Names, "as there is no context present". So the names
//! of an `UpdateResult` are expanded against that forwarding context before the
//! answer is merged with the broker's own.

use crate::{
    SentResponse,
    error::FederationError,
    outcome::{ForwardedWriteOutcome, TransportFailure},
};
use andromeda_jsonld::{
    context_spec::ContextSpec,
    engine::{processor::JsonLdEngine, term_lookup::PreparedTermContext},
};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use serde_json::Value;
use std::sync::Arc;
use tracing::warn;

/// The `UpdateResult` member listing the attributes that were updated
/// (clause 5.2.18).
const UPDATED_MEMBER: &str = "updated";
/// The `UpdateResult` member listing the attributes that were not updated
/// (clause 5.2.18).
const NOT_UPDATED_MEMBER: &str = "notUpdated";
/// The `NotUpdatedDetails` member naming the attribute (clause 5.2.19).
const ATTRIBUTE_NAME_MEMBER: &str = "attributeName";
/// The status of a partial success (clause 6.3.2).
const MULTI_STATUS: u16 = 207;

/// What the body of a context source's answer to a forwarded write holds.
#[derive(Debug)]
pub(crate) enum AnswerBody {
    /// The source sent no body.
    Absent,
    /// The source sent a JSON document.
    Json(Value),
    /// The source sent a body that is not JSON.
    Unreadable(serde_json::Error),
}

impl AnswerBody {
    /// Read the body a source sent.
    pub(crate) fn read(bytes: &[u8]) -> AnswerBody {
        if bytes.is_empty() {
            return AnswerBody::Absent;
        }
        match serde_json::from_slice(bytes) {
            Ok(value) => AnswerBody::Json(value),
            Err(error) => AnswerBody::Unreadable(error),
        }
    }
}

impl ForwardedWriteOutcome {
    /// Classify what `registration`'s context source answered a forwarded
    /// write with (clause 6.3.17).
    ///
    /// The status is the source's verdict on the operation whatever its body
    /// holds: a body that is not JSON cannot carry the clause 5.2.18
    /// `UpdateResult` or the problem detail it would add, so the outcome stands
    /// on its status alone, and the unreadable body is reported to operators
    /// with its typed parse failure.
    pub(crate) fn of_sent(registration: &Identifier, sent: &SentResponse) -> ForwardedWriteOutcome {
        let status = sent.status.as_u16();
        let body = match AnswerBody::read(&sent.body) {
            AnswerBody::Absent => None,
            AnswerBody::Json(value) => Some(value),
            AnswerBody::Unreadable(error) => {
                warn!(registration = %registration, status, %error, "context source answered a forwarded write with a body that is not JSON");
                None
            }
        };
        if sent.status.is_success() {
            ForwardedWriteOutcome::Completed { status, body }
        } else {
            ForwardedWriteOutcome::RemoteHttpFailure { status, body }
        }
    }
}

/// The outcome of a forwarded single-entity write with the attribute names of
/// a 207 `UpdateResult` expanded from `forwarding`, the context the request was
/// written in, into Fully Qualified Names.
///
/// A name that does not expand leaves the answer unreadable in Fully Qualified
/// Names, which is reported as [`TransportFailure::Unreadable`].
pub(crate) async fn with_full_update_result_names(engine: &JsonLdEngine, forwarding: &ContextSpec, outcome: ForwardedWriteOutcome) -> ForwardedWriteOutcome {
    let ForwardedWriteOutcome::Completed {
        status: MULTI_STATUS,
        body: Some(body),
    } = outcome
    else {
        return outcome;
    };
    let prepared = engine.prepare_term_context(forwarding);
    match full_update_result_names(engine, &prepared, body).await {
        Ok(body) => ForwardedWriteOutcome::Completed {
            status: MULTI_STATUS,
            body: Some(body),
        },
        Err(error) => ForwardedWriteOutcome::Transport(TransportFailure::Unreadable(Arc::new(error))),
    }
}

/// Expand the attribute names of a clause 5.2.18 `UpdateResult`.
async fn full_update_result_names(engine: &JsonLdEngine, forwarding: &PreparedTermContext, body: Value) -> Result<Value, FederationError> {
    let Value::Object(mut members) = body else {
        return Ok(body);
    };
    if let Some(Value::Array(updated)) = members.get_mut(UPDATED_MEMBER) {
        for name in updated.iter_mut() {
            if let Value::String(term) = name {
                engine.expand_term(term, forwarding).await?.as_str().clone_into(term);
            }
        }
    }
    if let Some(Value::Array(not_updated)) = members.get_mut(NOT_UPDATED_MEMBER) {
        for details in not_updated.iter_mut() {
            if let Some(Value::String(term)) = details.get_mut(ATTRIBUTE_NAME_MEMBER) {
                engine.expand_term(term, forwarding).await?.as_str().clone_into(term);
            }
        }
    }
    Ok(Value::Object(members))
}

#[cfg(test)]
mod tests {
    use crate::{
        outcome::{ForwardedWriteOutcome, TransportFailure},
        write_answer::{AnswerBody, with_full_update_result_names},
    };
    use andromeda_jsonld::{context_spec::ContextSpec, engine::processor::JsonLdEngine};
    use serde_json::json;

    fn engine() -> JsonLdEngine {
        JsonLdEngine::builder().allow_remote(false).build().expect("engine builds")
    }

    /// The context the forwarded request was written in: the registration's
    /// `jsonldContext` spells the attribute `velocity`.
    fn forwarding() -> ContextSpec {
        ContextSpec::from_body(json!({"velocity": "https://example.org/speed", "brand": "https://example.org/brand"}))
    }

    #[test]
    fn an_answer_body_is_absent_json_or_unreadable() {
        assert!(matches!(AnswerBody::read(b""), AnswerBody::Absent));
        assert!(matches!(AnswerBody::read(br#"{"notUpdated": []}"#), AnswerBody::Json(_)));
        assert!(matches!(AnswerBody::read(b"not JSON"), AnswerBody::Unreadable(_)));
    }

    /// Clause 6.3.6: a 207 body carries Fully Qualified Names, so the names a
    /// source wrote in the forwarding context are expanded before the answer is
    /// merged with the broker's own.
    #[tokio::test]
    async fn a_multi_status_update_result_is_read_back_in_full_names() {
        let outcome = ForwardedWriteOutcome::Completed {
            status: 207,
            body: Some(json!({
                "updated": ["brand"],
                "notUpdated": [{"attributeName": "velocity", "reason": "already exists"}]
            })),
        };

        let read = with_full_update_result_names(&engine(), &forwarding(), outcome).await;

        let (status, body) = match read {
            ForwardedWriteOutcome::Completed { status, body } => (status, body.expect("the update result stays")),
            other @ (ForwardedWriteOutcome::RemoteHttpFailure { .. }
            | ForwardedWriteOutcome::Transport(_)
            | ForwardedWriteOutcome::LoopDetected
            | ForwardedWriteOutcome::NotAdvertised
            | ForwardedWriteOutcome::UpsertFollowUpNotAdvertised) => (0, json!({ "unexpected": format!("{other:?}") })),
        };
        pretty_assertions::assert_eq!(status, 207, "{body}");
        pretty_assertions::assert_eq!(body["updated"], json!(["https://example.org/brand"]));
        pretty_assertions::assert_eq!(body["notUpdated"][0]["attributeName"], json!("https://example.org/speed"));
        pretty_assertions::assert_eq!(body["notUpdated"][0]["reason"], json!("already exists"));
    }

    /// Any other outcome carries no `UpdateResult` and is left as it is.
    #[tokio::test]
    async fn an_outcome_without_an_update_result_is_left_alone() {
        let engine = engine();
        for outcome in [
            ForwardedWriteOutcome::Completed { status: 204, body: None },
            ForwardedWriteOutcome::RemoteHttpFailure {
                status: 400,
                body: Some(json!({"attributeName": "velocity"})),
            },
            ForwardedWriteOutcome::Completed { status: 207, body: None },
        ] {
            let read = with_full_update_result_names(&engine, &forwarding(), outcome.clone()).await; // clone: compared below
            pretty_assertions::assert_eq!(format!("{read:?}"), format!("{outcome:?}"));
        }
        assert!(!matches!(
            with_full_update_result_names(&engine, &forwarding(), ForwardedWriteOutcome::Completed { status: 204, body: None }).await,
            ForwardedWriteOutcome::Transport(TransportFailure::Unreadable(_))
        ));
    }
}
