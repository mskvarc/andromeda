//! The clause 5.2.18 `UpdateResult` an attribute write answers with.
//!
//! Clauses 5.6.2.5 and 5.6.3.5 give the attribute update and append operations
//! the same two pieces of output data: a status code saying whether all or only
//! some of the attributes were written, and the list of attributes actually
//! written. Both handlers render that through this module so the two halves of
//! the payload cannot drift apart.

use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::{Value, json};
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
    /// An update found the entity did not already carry the attribute, which
    /// clause 5.6.2.4 makes the condition for leaving it untouched.
    #[strum(serialize = "Attribute not found")]
    AttributeNotFound,
}

/// Render a 207 Multi-Status carrying the clause 5.2.18 `UpdateResult`.
///
/// `requested` is the set of attributes the request asked to write; those it
/// does not list under `not_updated` are the ones "actually updated". Entries
/// in `remote_not_updated` are `UpdateResult` fragments already rendered by
/// forwarded context sources and are appended verbatim.
pub fn update_result_response(
    requested: &[AttributeName],
    not_updated: &[AttributeName],
    reason: NotUpdatedReason,
    remote_not_updated: Vec<Value>,
) -> Response {
    (StatusCode::MULTI_STATUS, Json(body(requested, not_updated, reason, remote_not_updated))).into_response()
}

/// The `UpdateResult` document itself, split out so it can be asserted directly.
fn body(requested: &[AttributeName], not_updated: &[AttributeName], reason: NotUpdatedReason, remote_not_updated: Vec<Value>) -> Value {
    let updated: Vec<&AttributeName> = requested.iter().filter(|attribute| !not_updated.contains(attribute)).collect();
    let description: &'static str = reason.into();
    let mut not_updated_details: Vec<Value> = not_updated
        .iter()
        .map(|attribute| json!({ "attributeName": attribute, "reason": description }))
        .collect();
    not_updated_details.extend(remote_not_updated);
    json!({ "updated": updated, "notUpdated": not_updated_details })
}

#[cfg(test)]
mod tests {
    use crate::entity::update_result::{NotUpdatedReason, body};
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    fn attribute(iri: &str) -> AttributeName {
        iri.parse().expect("the fixture names a valid attribute")
    }

    /// Clause 5.6.2.5: the output data is "the list of Attributes (Properties
    /// or Relationships) actually updated" -- the requested set minus whatever
    /// could not be written, never an empty list.
    #[test]
    fn attributes_that_were_written_are_reported_as_updated() {
        let requested = [attribute("https://example.org/speed"), attribute("https://example.org/brand")];
        let not_updated = [attribute("https://example.org/brand")];

        let rendered = body(&requested, &not_updated, NotUpdatedReason::AttributeNotFound, Vec::new());

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

        let rendered = body(&requested, &requested, NotUpdatedReason::AttributeNotFound, Vec::new());

        assert_eq!(rendered["updated"], json!([]));
    }

    /// An append refused under `noOverwrite` reports the clause 5.6.3.4 reason.
    #[test]
    fn an_append_refused_by_no_overwrite_reports_already_exists() {
        let requested = [attribute("https://example.org/speed")];

        let rendered = body(&requested, &requested, NotUpdatedReason::AlreadyExists, Vec::new());

        assert_eq!(rendered["notUpdated"][0]["reason"], json!("Already exists"));
    }

    /// `UpdateResult` fragments a forwarded context source already rendered are
    /// carried through untouched alongside the locally determined ones.
    #[test]
    fn forwarded_not_updated_entries_are_appended_verbatim() {
        let requested = [attribute("https://example.org/speed")];
        let remote = vec![json!({"attributeName": "https://example.org/remote", "reason": "Forwarded failure"})];

        let rendered = body(&requested, &[], NotUpdatedReason::AttributeNotFound, remote);

        assert_eq!(rendered["updated"], json!(["https://example.org/speed"]));
        assert_eq!(
            rendered["notUpdated"],
            json!([{"attributeName": "https://example.org/remote", "reason": "Forwarded failure"}])
        );
    }
}
