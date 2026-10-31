//! The clause 5.2.18 `UpdateResult` an attribute write answers with.
//!
//! Clauses 5.6.2.5 and 5.6.3.5 give the attribute update and append operations
//! the same two pieces of output data: a status code saying whether all or only
//! some of the attributes were written, and the list of attributes actually
//! written. Tables 6.6.3.1-2, 6.6.3.2-2, 6.7.3.1-2, 6.7.3.2-2 and 6.7.3.3-2
//! answer a distributed one of the five Attribute-level writes the same way:
//! when a forwarded part fails, "the result returned from each registration is
//! returned in a UpdateResult structure". Every attribute write renders its
//! answer through this module, so the local and the forwarded halves of the
//! payload cannot drift apart.

use crate::entity::LocalOutcome;
use andromeda_federation::{
    answer_body::AnswerBody,
    outcome::{ForwardedWriteOutcome, ForwardedWriteResult},
};
use andromeda_http_response::{
    batch_projection::forwarded_write_problem,
    problem_response::ProblemResponse,
    provision::{ProvisionVerdict, provision_verdict},
};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_model::update_result::{NotUpdatedDetails, UpdateResult};
use axum::{
    Json,
    http::{StatusCode, Uri as HttpUri},
    response::{IntoResponse, Response},
};
use strum::IntoStaticStr;

/// Why an attribute the request named was not written.
///
/// Clause 5.2.18 types `reason` as free-form human-readable text, so these are
/// descriptions rather than spec-defined tokens; the enum exists so a handler
/// cannot invent a third spelling of a reason that already has one.
#[derive(Clone, Copy, Debug, Eq, IntoStaticStr, PartialEq)]
pub enum NotUpdatedReason {
    /// An append under `noOverwrite` found the attribute already present.
    #[strum(serialize = "Already exists")]
    AlreadyExists,
    /// An update carried a `scope` the entity does not have, which clause
    /// 5.6.2.4 ignores.
    #[strum(serialize = "Attribute not found")]
    AttributeNotFound,
}

impl NotUpdatedReason {
    /// The clause 5.2.19 details of `attributes`, each left unwritten here for
    /// this reason.
    #[must_use]
    pub fn of(self, attributes: &[AttributeName]) -> Vec<NotUpdatedDetails> {
        let description: &'static str = self.into();
        attributes
            .iter()
            .map(|attribute| NotUpdatedDetails {
                attribute_name: attribute.clone(), // clone: the result owns the names it reports
                reason: description.to_owned(),
                registration_id: None,
            })
            .collect()
    }
}

/// What the local part of an attribute write did with the attributes it was
/// given.
pub struct LocalAttributeWrite<'a> {
    /// Whether the local part ran, and how it ended.
    pub outcome: &'a LocalOutcome,
    /// The attributes the local part was given, by their expanded names.
    pub requested: &'a [AttributeName],
    /// Those of `requested` the local part left unwritten without failing,
    /// each with why.
    pub unwritten: Vec<NotUpdatedDetails>,
}

/// Answer an attribute write from its local part and its forwarded parts: 204
/// when every attribute was written, and otherwise 207 Multi-Status carrying
/// the clause 5.2.18 `UpdateResult`.
///
/// Three things make the write "only some" (clauses 5.6.2.5 and 5.6.3.5): an
/// attribute the local part left unwritten, a forwarded part that failed, and
/// a context source that answered 207 for the part forwarded to it. Table
/// 6.6.3.1-2 makes a source's 207 exactly the statement that only some
/// attributes landed, so the status decides this even when the source omits
/// the body the table pairs with it.
///
/// # Errors
///
/// The complete operation failed, as [`provision_verdict`] decides.
pub fn attribute_write_response(local: LocalAttributeWrite<'_>, forwarded: &[ForwardedWriteResult], instance: &HttpUri) -> Result<Response, ProblemResponse> {
    let verdict = provision_verdict(forwarded, local.outcome, instance)?;
    let source_partly_wrote = forwarded.iter().any(|result| match &result.outcome {
        ForwardedWriteOutcome::Completed { status, .. } => *status == StatusCode::MULTI_STATUS.as_u16(),
        ForwardedWriteOutcome::RemoteHttpFailure { .. }
        | ForwardedWriteOutcome::Transport(_)
        | ForwardedWriteOutcome::LoopDetected
        | ForwardedWriteOutcome::NotAdvertised
        | ForwardedWriteOutcome::UpsertFollowUpNotAdvertised => false,
    });
    let result = update_result(local, forwarded);
    let complete = verdict == ProvisionVerdict::Succeeded && result.not_updated.is_empty() && !source_partly_wrote;
    if complete {
        return Ok(StatusCode::NO_CONTENT.into_response());
    }
    Ok((StatusCode::MULTI_STATUS, Json(result)).into_response())
}

/// The `UpdateResult` itself, split out so it can be asserted directly.
///
/// An attribute written by one part and refused by another is listed in both
/// members: each entry is one part's result. A forwarded failure names every
/// attribute the registration covers with the registration (Table 5.2.19-1
/// `registrationId`), and a source's own `notUpdated` entries are named with
/// the registration that reported them unless they already name one.
fn update_result(local: LocalAttributeWrite<'_>, forwarded: &[ForwardedWriteResult]) -> UpdateResult {
    let LocalAttributeWrite { outcome, requested, unwritten } = local;
    let mut result = UpdateResult::default();
    match outcome {
        LocalOutcome::NothingRemained => {}
        LocalOutcome::Processed(Ok(())) => {
            let written = requested
                .iter()
                .filter(|attribute| !unwritten.iter().any(|details| details.attribute_name == **attribute));
            push_updated(&mut result, written);
            result.not_updated.extend(unwritten);
        }
        LocalOutcome::Processed(Err(report)) => result.not_updated.extend(requested.iter().map(|attribute| NotUpdatedDetails {
            attribute_name: attribute.clone(), // clone: the result owns the names it reports
            reason: report.detail().to_owned(),
            registration_id: None,
        })),
    }
    for part in forwarded {
        match (&part.outcome, forwarded_write_problem(&part.registration_id, &part.outcome)) {
            (
                ForwardedWriteOutcome::Completed {
                    body: AnswerBody::UpdateResult(answered),
                    ..
                },
                _,
            ) => {
                push_updated(&mut result, answered.updated.iter());
                // clone: the answer owns the entries the forwarded result still holds
                result.not_updated.extend(answered.not_updated.iter().map(|details| NotUpdatedDetails {
                    attribute_name: details.attribute_name.clone(),
                    reason: details.reason.clone(),
                    registration_id: Some(details.registration_id.as_ref().unwrap_or(&part.registration_id).clone()),
                }));
            }
            (_, None) => push_updated(&mut result, part.attributes.iter()),
            (_, Some(problem)) => result.not_updated.extend(part.attributes.iter().map(|attribute| NotUpdatedDetails {
                attribute_name: attribute.clone(),                   // clone: the result owns the names it reports
                reason: problem.detail.clone(),                      // clone: every attribute the failed part covers carries its reason
                registration_id: Some(part.registration_id.clone()), // clone: the result owns the registration it names
            })),
        }
    }
    result
}

/// Add `written` to the attributes `result` reports updated, each once.
fn push_updated<'a>(result: &mut UpdateResult, written: impl Iterator<Item = &'a AttributeName>) {
    for attribute in written {
        if !result.updated.contains(attribute) {
            result.updated.push(attribute.clone()); // clone: the result owns the names it reports
        }
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::entity::update_result::{LocalAttributeWrite, NotUpdatedReason, attribute_write_response, update_result};
    use andromeda_federation::{
        answer_body::AnswerBody,
        outcome::{ForwardedWriteOutcome, ForwardedWriteResult},
    };
    use andromeda_http_response::{classified_report::ClassifiedReport, problem_response::ProblemResponse, provision::LocalOutcome};
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use andromeda_ngsi_ld_model::{
        registration::forwarding_mode::ForwardingMode,
        update_result::{NotUpdatedDetails, UpdateResult},
    };
    use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
    use axum::{
        body::to_bytes,
        http::Uri as HttpUri,
        response::{IntoResponse, Response},
    };
    use pretty_assertions::assert_eq;
    use serde_json::{Value, json};
    use std::sync::Arc;

    const REGISTRATION: &str = "urn:ngsi-ld:ContextSourceRegistration:1";

    fn attribute(iri: &str) -> AttributeName {
        iri.parse().expect("the fixture names a valid attribute")
    }

    fn written(result: &UpdateResult) -> Value {
        serde_json::to_value(result).expect("writes")
    }

    fn instance() -> HttpUri {
        HttpUri::from_static("/ngsi-ld/v1/entities/urn:ngsi-ld:Vehicle:1/attrs")
    }

    fn forwarded(mode: ForwardingMode, outcome: ForwardedWriteOutcome, attributes: &[&str]) -> ForwardedWriteResult {
        ForwardedWriteResult {
            registration_id: REGISTRATION.parse().expect("valid URI"),
            mode,
            outcome,
            attributes: attributes.iter().map(|name| attribute(name)).collect(),
        }
    }

    fn source_error(status: u16) -> ForwardedWriteOutcome {
        ForwardedWriteOutcome::RemoteHttpFailure {
            status,
            body: AnswerBody::Absent,
        }
    }

    fn local_success<'a>(outcome: &'a LocalOutcome, requested: &'a [AttributeName], unwritten: Vec<NotUpdatedDetails>) -> LocalAttributeWrite<'a> {
        LocalAttributeWrite { outcome, requested, unwritten }
    }

    /// Clause 5.6.2.5: the output data is "the list of Attributes (Properties
    /// or Relationships) actually updated" -- the requested set minus whatever
    /// could not be written, never an empty list.
    #[test]
    fn attributes_that_were_written_are_reported_as_updated() {
        let requested = [attribute("https://example.org/speed"), attribute("https://example.org/brand")];
        let unwritten = NotUpdatedReason::AttributeNotFound.of(&[attribute("https://example.org/brand")]);
        let outcome = LocalOutcome::Processed(Ok(()));

        let rendered = written(&update_result(local_success(&outcome, &requested, unwritten), &[]));

        assert_eq!(rendered["updated"], json!(["https://example.org/speed"]));
        assert_eq!(
            rendered["notUpdated"],
            json!([{"attributeName": "https://example.org/brand", "reason": "Attribute not found"}])
        );
    }

    /// Every requested attribute failing leaves `updated` empty, which is the
    /// only case in which clause 5.2.18 admits an empty list.
    #[test]
    fn nothing_written_reports_no_updated_attributes() {
        let requested = [attribute("https://example.org/speed")];
        let outcome = LocalOutcome::Processed(Ok(()));

        let rendered = written(&update_result(
            local_success(&outcome, &requested, NotUpdatedReason::AttributeNotFound.of(&requested)),
            &[],
        ));

        assert_eq!(rendered["updated"], json!([]));
    }

    /// An append refused under `noOverwrite` reports the clause 5.6.3.4 reason.
    #[test]
    fn an_append_refused_by_no_overwrite_reports_already_exists() {
        let requested = [attribute("https://example.org/speed")];
        let outcome = LocalOutcome::Processed(Ok(()));

        let rendered = written(&update_result(
            local_success(&outcome, &requested, NotUpdatedReason::AlreadyExists.of(&requested)),
            &[],
        ));

        assert_eq!(rendered["notUpdated"][0]["reason"], json!("Already exists"));
    }

    /// Table 6.6.3.1-2 gives a context source's own 207 the same body, so the
    /// part it refused is relayed, named with the registration that refused it.
    #[test]
    fn a_source_s_not_updated_entries_are_relayed_with_its_registration() {
        let requested = [attribute("https://example.org/speed")];
        let outcome = LocalOutcome::Processed(Ok(()));
        let answered = UpdateResult {
            updated: vec![attribute("https://example.org/fuel")],
            not_updated: vec![NotUpdatedDetails {
                attribute_name: attribute("https://example.org/remote"),
                reason: "Forwarded failure".to_owned(),
                registration_id: None,
            }],
        };
        let source = forwarded(
            ForwardingMode::Inclusive,
            ForwardedWriteOutcome::Completed {
                status: 207,
                body: AnswerBody::UpdateResult(answered),
            },
            &["https://example.org/fuel", "https://example.org/remote"],
        );

        let rendered = written(&update_result(local_success(&outcome, &requested, Vec::new()), &[source]));

        assert_eq!(rendered["updated"], json!(["https://example.org/speed", "https://example.org/fuel"]));
        assert_eq!(
            rendered["notUpdated"],
            json!([{"attributeName": "https://example.org/remote", "reason": "Forwarded failure", "registrationId": REGISTRATION}])
        );
    }

    /// Tables 6.6.3.1-2 and 6.7.3.1-2: a failed forwarded part names every
    /// attribute its registration covers in `notUpdated`, with the
    /// registration (Table 5.2.19-1), while what the local part wrote is
    /// `updated`.
    #[test]
    fn a_failed_forwarded_part_names_its_attributes_and_registration() {
        let requested = [attribute("https://example.org/colour")];
        let outcome = LocalOutcome::Processed(Ok(()));
        let refused = forwarded(ForwardingMode::Exclusive, ForwardedWriteOutcome::NotAdvertised, &["https://example.org/speed"]);

        let rendered = written(&update_result(local_success(&outcome, &requested, Vec::new()), &[refused]));

        assert_eq!(rendered["updated"], json!(["https://example.org/colour"]));
        assert_eq!(rendered["notUpdated"][0]["attributeName"], json!("https://example.org/speed"));
        assert_eq!(rendered["notUpdated"][0]["registrationId"], json!(REGISTRATION));
        assert!(
            rendered["notUpdated"][0]["reason"]
                .as_str()
                .is_some_and(|reason| reason.contains("does not advertise")),
            "{rendered}"
        );
    }

    /// An inclusive attribute written here and refused by its source is one
    /// result per part: `updated` from the local part, `notUpdated` from the
    /// source.
    #[test]
    fn an_attribute_written_here_and_refused_by_its_source_is_reported_by_both_parts() {
        let speed = [attribute("https://example.org/speed")];
        let outcome = LocalOutcome::Processed(Ok(()));
        let failed = forwarded(ForwardingMode::Inclusive, source_error(500), &["https://example.org/speed"]);

        let rendered = written(&update_result(local_success(&outcome, &speed, Vec::new()), &[failed]));

        assert_eq!(rendered["updated"], json!(["https://example.org/speed"]));
        assert_eq!(rendered["notUpdated"].as_array().map(Vec::len), Some(1));
        assert_eq!(rendered["notUpdated"][0]["registrationId"], json!(REGISTRATION));
    }

    /// A local part that failed names its attributes with the local reason and
    /// no registration; what a source wrote is `updated`.
    #[test]
    fn a_failed_local_part_names_its_attributes_without_a_registration() {
        let speed = [attribute("https://example.org/speed")];
        let outcome = LocalOutcome::Processed(Err(ClassifiedReport::public(NgsiLdErrorType::ResourceNotFound, "Attribute speed not found")));
        let source = forwarded(
            ForwardingMode::Inclusive,
            ForwardedWriteOutcome::Completed {
                status: 204,
                body: AnswerBody::Absent,
            },
            &["https://example.org/speed"],
        );

        let rendered = written(&update_result(local_success(&outcome, &speed, Vec::new()), &[source]));

        assert_eq!(rendered["updated"], json!(["https://example.org/speed"]));
        assert_eq!(
            rendered["notUpdated"],
            json!([{"attributeName": "https://example.org/speed", "reason": "Attribute speed not found"}])
        );
    }

    /// A problem detail the source answered with gives the reason.
    #[test]
    fn a_source_s_problem_detail_is_the_reason() {
        let outcome = LocalOutcome::NothingRemained;
        let problem = json!({"type": "https://uri.etsi.org/ngsi-ld/errors/InternalError", "title": "Internal error", "detail": "disk full"});
        let failed = forwarded(
            ForwardingMode::Redirect,
            ForwardedWriteOutcome::RemoteHttpFailure {
                status: 500,
                body: AnswerBody::Text(Arc::from(problem.to_string().into_bytes())),
            },
            &["https://example.org/speed"],
        );

        let rendered = written(&update_result(local_success(&outcome, &[], Vec::new()), &[failed]));

        assert_eq!(rendered["notUpdated"][0]["reason"], json!("disk full"));
    }

    fn status_of(response: Result<Response, ProblemResponse>) -> u16 {
        match response {
            Ok(response) => response.status().as_u16(),
            Err(problem) => problem.into_response().status().as_u16(),
        }
    }

    /// Table 6.6.3.1-2: 204 only when every attribute was written.
    #[test]
    fn every_attribute_written_everywhere_is_no_content() {
        let speed = [attribute("https://example.org/speed")];
        let outcome = LocalOutcome::Processed(Ok(()));
        let source = forwarded(
            ForwardingMode::Inclusive,
            ForwardedWriteOutcome::Completed {
                status: 204,
                body: AnswerBody::Absent,
            },
            &["https://example.org/speed"],
        );

        let response = attribute_write_response(local_success(&outcome, &speed, Vec::new()), &[source], &instance());

        assert_eq!(status_of(response), 204);
    }

    /// Table 6.6.3.1-2: a source's 207 says only some of its part landed, even
    /// without a body.
    #[test]
    fn a_source_answering_207_makes_the_write_a_207() {
        let speed = [attribute("https://example.org/speed")];
        let outcome = LocalOutcome::Processed(Ok(()));
        let source = forwarded(
            ForwardingMode::Inclusive,
            ForwardedWriteOutcome::Completed {
                status: 207,
                body: AnswerBody::Absent,
            },
            &["https://example.org/speed"],
        );

        let response = attribute_write_response(local_success(&outcome, &speed, Vec::new()), &[source], &instance());

        assert_eq!(status_of(response), 207);
    }

    /// Tables 6.6.3.1-2 to 6.7.3.3-2: the 207 of a forwarded part that failed
    /// is an `UpdateResult`, never a `BatchOperationResult`.
    #[tokio::test]
    async fn the_207_of_a_failed_forwarded_part_is_an_update_result() {
        let speed = [attribute("https://example.org/speed")];
        let outcome = LocalOutcome::Processed(Ok(()));
        let failed = forwarded(ForwardingMode::Inclusive, source_error(500), &["https://example.org/speed"]);

        let response = attribute_write_response(local_success(&outcome, &speed, Vec::new()), &[failed], &instance()).expect("a partial success");

        assert_eq!(response.status().as_u16(), 207);
        let body = to_bytes(response.into_body(), usize::MAX).await.expect("a body");
        let body: Value = serde_json::from_slice(&body).expect("JSON");
        assert_eq!(body["updated"], json!(["https://example.org/speed"]));
        assert_eq!(body["notUpdated"][0]["registrationId"], json!(REGISTRATION));
        assert!(body.get("success").is_none() && body.get("errors").is_none(), "{body}");
    }

    /// Clause 5.6.4.4: a single proxied source failing alone is the clause
    /// 6.3.17 error, not a 207.
    #[test]
    fn a_lone_proxied_source_failing_is_the_operation_s_error() {
        let failed = forwarded(ForwardingMode::Redirect, source_error(500), &["https://example.org/speed"]);

        let response = attribute_write_response(local_success(&LocalOutcome::NothingRemained, &[], Vec::new()), &[failed], &instance());

        assert_eq!(status_of(response), 502);
    }
}
