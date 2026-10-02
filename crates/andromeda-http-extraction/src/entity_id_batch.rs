//! The request body of a batch delete: an array of Entity IDs (clause
//! 5.6.10).
//!
//! The body is decoded item by item rather than as one typed array, so an
//! `id` that is a URI too long to be stored is reported against itself as a
//! clause 5.2.17 `BatchEntityError` while the rest of the batch is deleted.
//! Only an item with no URI to report against fails the request.

use crate::{
    batch_array::batch_items,
    batch_body_refusal::{BatchBodyRefusal, BatchMembers},
    batch_item_refusal::BatchItemRefusal,
};
use andromeda_http_response::problem_response::ProblemResponse;
use andromeda_ngsi_ld_lexicon::{
    identifier::{Identifier, IdentifierParseError},
    uri::Uri,
};
use andromeda_ngsi_ld_problem_details::batch_operation::BatchEntityError;
use axum::{body::Bytes, http::Uri as HttpUri};
use serde_json::Value;

/// The Entity IDs a batch delete names, and the ones refused per entity.
#[derive(Debug)]
pub struct EntityIdBatch {
    /// The identifiers to delete, in array order.
    pub ids: Vec<Identifier>,
    /// One clause 5.2.17 error per item refused against its own `id`.
    pub refused: Vec<BatchEntityError>,
}

/// Decode the body of a batch delete into the identifiers it names.
///
/// The body was counted against the request's JSON value limit when it was
/// read (see [`RequestPayload`](crate::request_payload::RequestPayload)).
///
/// # Errors
///
/// Returns a [`ProblemResponse`] for the [`BatchBodyRefusal`] that fails the
/// request as a whole: a body that is not JSON, not an array or an empty array
/// (clauses 5.5.4 and 5.6.10.4), a null item, an item that is not a string
/// (clause 5.6.10.3), or a string that is not a URI (Table 5.2.17-1).
pub fn parse_batch_entity_ids(body: &Bytes, instance: &HttpUri) -> Result<EntityIdBatch, ProblemResponse> {
    entity_id_batch(body).map_err(|refusal| refusal.into_problem(instance.clone())) // clone: the refusal names the request it answers
}

/// The identifiers `body` names, or the refusal that fails the request.
fn entity_id_batch(body: &Bytes) -> Result<EntityIdBatch, BatchBodyRefusal> {
    let items = batch_items::<Value>(body, BatchMembers::EntityIds)?;
    let mut batch = EntityIdBatch {
        ids: Vec::with_capacity(items.len()),
        refused: Vec::new(),
    };
    for (index, item) in items.into_iter().enumerate() {
        let raw = match item {
            Value::String(raw) => raw,
            Value::Null => return Err(BatchBodyRefusal::NullItem { index }),
            Value::Bool(_) | Value::Number(_) | Value::Array(_) | Value::Object(_) => return Err(BatchBodyRefusal::NotAnEntityId { index }),
        };
        match raw.parse::<Identifier>() {
            Ok(id) => batch.ids.push(id),
            Err(IdentifierParseError::Malformed(source)) => return Err(BatchBodyRefusal::MalformedId { index, source }),
            // The `id` is a URI past the identifier bound; it is parsed once
            // more as the URI the error is reported against, which only this
            // refused item pays for.
            Err(IdentifierParseError::TooLong(too_long)) => {
                let entity_id = raw.parse::<Uri>().map_err(|source| BatchBodyRefusal::MalformedId { index, source })?;
                batch.refused.push(BatchItemRefusal::IdTooLong(too_long).against(entity_id));
            }
        }
    }
    Ok(batch)
}

#[cfg(test)]
mod tests {
    use crate::entity_id_batch::parse_batch_entity_ids;
    use andromeda_ngsi_ld_lexicon::identifier_length::IDENTIFIER_BYTE_LIMIT;
    use andromeda_ngsi_ld_problem_details::{batch_operation::BatchErrorKind, error_type::NgsiLdErrorType, problem_detail::ProblemClassification};
    use axum::{body::Bytes, http::Uri};

    fn instance() -> Uri {
        Uri::from_static("/ngsi-ld/v1/entityOperations/delete")
    }

    /// A URN of exactly `length` octets.
    fn urn_of_length(length: usize) -> String {
        let prefix = "urn:ngsi-ld:Vehicle:";
        format!("{prefix}{}", "a".repeat(length - prefix.len()))
    }

    /// The category and detail the request is refused with.
    fn refusal_of(body: &str) -> (NgsiLdErrorType, String) {
        match parse_batch_entity_ids(&Bytes::from(body.to_owned()), &instance()) {
            Ok(batch) => panic!("the request was to fail as a whole, parsed {batch:?}"),
            Err(refusal) => (refusal.problem().error_type(), refusal.problem().detail().to_owned()),
        }
    }

    #[test]
    fn well_formed_ids_are_all_identifiers() {
        let batch = parse_batch_entity_ids(&Bytes::from_static(br#"["urn:ngsi-ld:Vehicle:a","urn:ngsi-ld:Vehicle:b"]"#), &instance()).expect("a batch of ids");
        pretty_assertions::assert_eq!(
            batch.ids.iter().map(ToString::to_string).collect::<Vec<_>>(),
            vec!["urn:ngsi-ld:Vehicle:a", "urn:ngsi-ld:Vehicle:b"]
        );
        pretty_assertions::assert_eq!(batch.refused, Vec::new());
    }

    /// Table 5.2.17-1 sets no length on `entityId`: an `id` too long to be
    /// stored is reported against itself and its neighbours are deleted.
    #[test]
    fn an_over_long_id_is_refused_against_itself() {
        let long_id = urn_of_length(IDENTIFIER_BYTE_LIMIT + 1);
        let body = format!(r#"["urn:ngsi-ld:Vehicle:a","{long_id}"]"#);
        let batch = parse_batch_entity_ids(&Bytes::from(body), &instance()).expect("a batch of ids");
        pretty_assertions::assert_eq!(batch.ids.iter().map(ToString::to_string).collect::<Vec<_>>(), vec!["urn:ngsi-ld:Vehicle:a"]);
        pretty_assertions::assert_eq!(batch.refused.len(), 1);
        pretty_assertions::assert_eq!(batch.refused[0].entity_id.as_str(), long_id.as_str());
        pretty_assertions::assert_eq!(
            batch.refused[0].error.error_type,
            BatchErrorKind::Known(ProblemClassification::Category(NgsiLdErrorType::BadRequestData))
        );
        assert!(batch.refused[0].error.detail.contains("1025 bytes"), "{}", batch.refused[0].error.detail);
    }

    /// Clauses 5.6.10.3 and 5.6.10.4 and Table 5.2.17-1: an item with no URI to
    /// report against fails the request as `BadRequestData`, naming the item.
    #[test]
    fn an_item_with_no_uri_to_report_against_fails_the_request() {
        for (body, detail) in [
            (r#"["urn:ngsi-ld:Vehicle:a",null]"#, "batch item 1 is null"),
            (r#"["urn:ngsi-ld:Vehicle:a",42]"#, "batch item 1 is not an entity id string"),
            (r#"["urn:ngsi-ld:Vehicle:a",{}]"#, "batch item 1 is not an entity id string"),
            (
                r#"["urn:ngsi-ld:Vehicle:a","not a uri"]"#,
                "the 'id' of batch item 1 is not a URI: invalid URI: not a valid URN, URL or IRI",
            ),
        ] {
            pretty_assertions::assert_eq!(refusal_of(body), (NgsiLdErrorType::BadRequestData, detail.to_owned()), "{body}");
        }
    }

    /// Clause 5.5.4: only a body that is not JSON is `InvalidRequest`.
    #[test]
    fn the_body_shape_refusals_carry_the_clause_5_5_4_categories() {
        pretty_assertions::assert_eq!(
            refusal_of("[]"),
            (
                NgsiLdErrorType::BadRequestData,
                "the request body must be a non-empty JSON array of entity ids".to_owned()
            )
        );
        pretty_assertions::assert_eq!(
            refusal_of("{}"),
            (
                NgsiLdErrorType::BadRequestData,
                "the request body must be a JSON array of entity ids".to_owned()
            )
        );
        pretty_assertions::assert_eq!(refusal_of("[,").0, NgsiLdErrorType::InvalidRequest);
    }
}
