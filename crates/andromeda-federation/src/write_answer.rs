//! Reading what a context source answered a forwarded write with (clauses
//! 4.3.6.6, 5.2.18, 6.3.6 and 6.3.17).
//!
//! The status is the source's verdict on the operation. The body, when there
//! is one, is kept as the text the source wrote: a 207 `UpdateResult`, a batch
//! result, a problem detail or a created resource, each read typed by the one
//! that needs it. A source writes the attribute names of an `UpdateResult` in
//! the context the forwarded request named, while clause 6.3.6 has a 207 body
//! carry Fully Qualified Names, "as there is no context present"; so the
//! `UpdateResult` of a single-entity write is read once, its names expanded
//! against that forwarding context, before the answer is merged with the
//! broker's own.

use crate::{
    answer_body::AnswerBody,
    error::FederationError,
    federation_invariant::FederationInvariant,
    outcome::{ForwardedWriteOutcome, TransportFailure},
    source_exchange::SentResponse,
};
use andromeda_jsonld::{
    context_spec::ContextSpec,
    engine::{processor::JsonLdEngine, term_lookup::PreparedTermContext},
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier};
use andromeda_ngsi_ld_model::update_result::UpdateResult;
use serde::de::IgnoredAny;
use std::{str::FromStr, sync::Arc};
use tracing::warn;

/// The status of a partial success (clause 6.3.2).
const MULTI_STATUS: u16 = 207;

/// Read the body a source sent: absent when empty, refused to operators when
/// it is no JSON.
fn answer_body(registration: &Identifier, status: u16, bytes: &[u8]) -> AnswerBody {
    if bytes.is_empty() {
        return AnswerBody::Absent;
    }
    match serde_json::from_slice::<IgnoredAny>(bytes) {
        Ok(_) => AnswerBody::Text(Arc::from(bytes)),
        Err(error) => {
            warn!(registration = %registration, status, %error, "context source answered a forwarded write with a body that is not JSON");
            AnswerBody::Absent
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
        let body = answer_body(registration, status, &sent.body);
        if sent.status.is_success() {
            ForwardedWriteOutcome::Completed { status, body }
        } else {
            ForwardedWriteOutcome::RemoteHttpFailure { status, body }
        }
    }
}

/// The outcome of a forwarded single-entity write with its 207 `UpdateResult`
/// read, its attribute names expanded from `forwarding`, the context the
/// request was written in, into Fully Qualified Names.
///
/// A body that is no `UpdateResult`, or a name that does not expand, leaves
/// the answer unreadable in Fully Qualified Names, which is reported as
/// [`TransportFailure::Unreadable`].
pub(crate) async fn with_full_update_result_names(engine: &JsonLdEngine, forwarding: &ContextSpec, outcome: ForwardedWriteOutcome) -> ForwardedWriteOutcome {
    let ForwardedWriteOutcome::Completed {
        status: MULTI_STATUS,
        body: AnswerBody::Text(text),
    } = outcome
    else {
        return outcome;
    };
    let prepared = engine.prepare_term_context(forwarding);
    let read = match serde_json::from_slice::<UpdateResult>(&text) {
        Ok(result) => full_update_result_names(engine, &prepared, result).await,
        Err(error) => Err(FederationError::MultiStatusUnreadable(error)),
    };
    match read {
        Ok(result) => ForwardedWriteOutcome::Completed {
            status: MULTI_STATUS,
            body: AnswerBody::UpdateResult(result),
        },
        Err(error) => ForwardedWriteOutcome::Transport(TransportFailure::Unreadable(Arc::new(error))),
    }
}

/// Expand the attribute names of a clause 5.2.18 `UpdateResult`.
async fn full_update_result_names(engine: &JsonLdEngine, forwarding: &PreparedTermContext, mut result: UpdateResult) -> Result<UpdateResult, FederationError> {
    for name in &mut result.updated {
        *name = expanded(engine, forwarding, name).await?;
    }
    for details in &mut result.not_updated {
        details.attribute_name = expanded(engine, forwarding, &details.attribute_name).await?;
    }
    Ok(result)
}

/// One attribute name, expanded against the forwarding context.
async fn expanded(engine: &JsonLdEngine, forwarding: &PreparedTermContext, name: &AttributeName) -> Result<AttributeName, FederationError> {
    let iri = engine.expand_term(name.as_str(), forwarding).await?;
    AttributeName::from_str(iri.as_str()).map_err(|error| FederationError::from(FederationInvariant::ForwardedName(error)))
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    clippy::panic,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        answer_body::AnswerBody,
        error::FederationError,
        outcome::{ForwardedWriteOutcome, TransportFailure},
        write_answer::with_full_update_result_names,
    };
    use andromeda_json_literal::JsonLiteral;
    use andromeda_jsonld::{context_spec::ContextSpec, engine::processor::JsonLdEngine};
    use serde_json::json;
    use std::sync::Arc;

    fn engine() -> JsonLdEngine {
        JsonLdEngine::builder().allow_remote(false).build().expect("engine builds")
    }

    /// The context the forwarded request was written in: the registration's
    /// `jsonldContext` spells the attribute `velocity`.
    fn forwarding() -> ContextSpec {
        ContextSpec::from_body(
            JsonLiteral::admit(&json!({"velocity": "https://example.org/speed", "brand": "https://example.org/brand"}).to_string())
                .expect("an admissible literal"),
        )
    }

    fn text(body: &serde_json::Value) -> AnswerBody {
        AnswerBody::Text(Arc::from(body.to_string().into_bytes()))
    }

    /// Clause 6.3.6: a 207 body carries Fully Qualified Names, so the names a
    /// source wrote in the forwarding context are expanded before the answer is
    /// merged with the broker's own.
    #[tokio::test]
    async fn a_multi_status_update_result_is_read_back_in_full_names() {
        let outcome = ForwardedWriteOutcome::Completed {
            status: 207,
            body: text(&json!({
                "updated": ["brand"],
                "notUpdated": [{"attributeName": "velocity", "reason": "already exists"}]
            })),
        };

        let read = with_full_update_result_names(&engine(), &forwarding(), outcome).await;

        let ForwardedWriteOutcome::Completed {
            status: 207,
            body: AnswerBody::UpdateResult(result),
        } = read
        else {
            panic!("the update result is read: {read:?}");
        };
        pretty_assertions::assert_eq!(result.updated[0].as_str(), "https://example.org/brand");
        pretty_assertions::assert_eq!(result.not_updated[0].attribute_name.as_str(), "https://example.org/speed");
        pretty_assertions::assert_eq!(result.not_updated[0].reason, "already exists");
    }

    /// Any other outcome carries no `UpdateResult` and is left as it is.
    #[tokio::test]
    async fn an_outcome_without_an_update_result_is_left_alone() {
        let engine = engine();
        for outcome in [
            ForwardedWriteOutcome::Completed {
                status: 204,
                body: AnswerBody::Absent,
            },
            ForwardedWriteOutcome::RemoteHttpFailure {
                status: 400,
                body: text(&json!({"attributeName": "velocity"})),
            },
            ForwardedWriteOutcome::Completed {
                status: 207,
                body: AnswerBody::Absent,
            },
        ] {
            let read = with_full_update_result_names(&engine, &forwarding(), outcome.clone()).await; // clone: compared below
            pretty_assertions::assert_eq!(format!("{read:?}"), format!("{outcome:?}"));
        }
        assert!(!matches!(
            with_full_update_result_names(
                &engine,
                &forwarding(),
                ForwardedWriteOutcome::Completed {
                    status: 204,
                    body: AnswerBody::Absent
                }
            )
            .await,
            ForwardedWriteOutcome::Transport(TransportFailure::Unreadable(_))
        ));
    }

    /// A 207 body that is no `UpdateResult` cannot be read in Fully Qualified
    /// Names.
    #[tokio::test]
    async fn a_multi_status_body_that_is_no_update_result_is_unreadable() {
        let outcome = ForwardedWriteOutcome::Completed {
            status: 207,
            body: text(&json!({"success": ["urn:ngsi-ld:Vehicle:1"], "errors": []})),
        };
        let read = with_full_update_result_names(&engine(), &forwarding(), outcome).await;
        let ForwardedWriteOutcome::Transport(TransportFailure::Unreadable(error)) = read else {
            panic!("the answer is unreadable: {read:?}");
        };
        assert!(matches!(error.as_ref(), FederationError::MultiStatusUnreadable(_)), "{error:?}");
        assert!(error.to_string().starts_with("the 207 body is not an UpdateResult: "), "{error}");
    }
}
