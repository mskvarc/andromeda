//! The refusals that fail a batch operation as a whole rather than per entity.
//!
//! Clauses 5.6.7.4 to 5.6.10.4 and 5.6.20.4 report every failure against the
//! entity it concerns, in a clause 5.2.17 `BatchEntityError`. What remains here
//! is what has no entity to be reported against: a body holding more JSON
//! values than the broker accepts in one request, a body that is not JSON, a
//! body that is not a non-empty array, a null item, and an item without a URI
//! to stand in the error's `entityId`, which Table 5.2.17-1 requires to be a
//! valid URI.

use crate::{json_value_budget::RequestJsonValueBudgetExceeded, not_json_document::NotJsonDocument};
use andromeda_http_response::request_refusal::RequestRefusal;
use andromeda_ngsi_ld_lexicon::uri::UriParseError;
use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
use strum::Display;
use thiserror::Error;

/// What the array of a batch request holds.
#[derive(Clone, Copy, Debug, Display, Eq, PartialEq)]
pub enum BatchMembers {
    /// Entities, for create, upsert, update and merge (clauses 5.6.7 to 5.6.9
    /// and 5.6.20).
    #[strum(serialize = "entities")]
    Entities,
    /// Entity identifiers, for delete (clause 5.6.10).
    #[strum(serialize = "entity ids")]
    EntityIds,
}

/// Why a batch request fails as a whole rather than per entity.
///
/// No variant echoes an `id` the client sent: an item is named by its
/// zero-based position in the array, which is what a client needs to find it,
/// and an `id` that failed to parse can be as long as the request body.
#[derive(Debug, Error)]
pub enum BatchBodyRefusal {
    /// Clause 6.3.2: the payload body holds more JSON values than the broker
    /// accepts in one request.
    #[error(transparent)]
    TooManyValues(#[from] RequestJsonValueBudgetExceeded),
    /// Clause 5.5.4: the payload body is not a JSON document.
    #[error(transparent)]
    NotJson(#[from] NotJsonDocument),
    /// Clauses 5.6.7.3 to 5.6.10.3 and 5.6.20.3: the body is not an array.
    #[error("the request body must be a JSON array of {members}")]
    NotAnArray {
        /// What the array was to hold.
        members: BatchMembers,
    },
    /// Clauses 5.6.7.4 to 5.6.10.4: the array is empty.
    #[error("the request body must be a non-empty JSON array of {members}")]
    Empty {
        /// What the array was to hold.
        members: BatchMembers,
    },
    /// Clauses 5.6.7.4 to 5.6.10.4: an item is null.
    #[error("batch item {index} is null")]
    NullItem {
        /// The item's zero-based position in the array.
        index: usize,
    },
    /// Table 5.2.4-1: an entity carries no `id` string, so nothing can be
    /// reported against it.
    #[error("batch item {index} has no 'id' string")]
    MissingId {
        /// The item's zero-based position in the array.
        index: usize,
    },
    /// Table 5.2.17-1: an `entityId` must be a valid URI, so an `id` that is
    /// not one leaves the error nothing to name.
    #[error("the 'id' of batch item {index} is not a URI: {source}")]
    MalformedId {
        /// The item's zero-based position in the array.
        index: usize,
        /// Why the `id` is not a URI.
        #[source]
        source: UriParseError,
    },
    /// Clause 5.6.10.3: a batch delete item is not an Entity ID string.
    #[error("batch item {index} is not an entity id string")]
    NotAnEntityId {
        /// The item's zero-based position in the array.
        index: usize,
    },
}

impl RequestRefusal for BatchBodyRefusal {
    /// The clause 5.5.2 category the refusal is reported as.
    ///
    /// Clause 5.5.4 raises `InvalidRequest` only for a payload body that "is
    /// not a valid JSON document"; data that is not syntactically correct
    /// according to the API data type definitions -- the array shape, a null
    /// item, an `id` that is absent or not a URI -- is `BadRequestData`; and a
    /// body larger than the broker accepts is the clause 6.3.2 "Request Entity
    /// too large".
    fn error_type(&self) -> NgsiLdErrorType {
        match self {
            BatchBodyRefusal::TooManyValues(_) => NgsiLdErrorType::RequestEntityTooLarge,
            BatchBodyRefusal::NotJson(refusal) => refusal.error_type(),
            BatchBodyRefusal::NotAnArray { .. }
            | BatchBodyRefusal::Empty { .. }
            | BatchBodyRefusal::NullItem { .. }
            | BatchBodyRefusal::MissingId { .. }
            | BatchBodyRefusal::MalformedId { .. }
            | BatchBodyRefusal::NotAnEntityId { .. } => NgsiLdErrorType::BadRequestData,
        }
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        batch_body_refusal::{BatchBodyRefusal, BatchMembers},
        not_json_document::NotJsonDocument,
    };
    use andromeda_http_response::request_refusal::RequestRefusal;
    use andromeda_ngsi_ld_lexicon::uri::UriParseError;
    use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
    use axum::http::{StatusCode, Uri};

    fn not_json() -> BatchBodyRefusal {
        BatchBodyRefusal::NotJson(NotJsonDocument(
            serde_json::from_str::<serde_json::Value>("[,").expect_err("not a JSON document"),
        ))
    }

    /// The batch reader words a body that is no JSON as every other reader does.
    #[test]
    fn a_body_that_is_not_json_is_worded_as_every_other_reader_words_it() {
        assert!(not_json().to_string().starts_with("request body is not a JSON document:"), "{}", not_json());
    }

    /// Clause 5.5.4: only a body that is not JSON is `InvalidRequest`.
    #[test]
    fn only_a_body_that_is_not_json_is_an_invalid_request() {
        pretty_assertions::assert_eq!(not_json().error_type(), NgsiLdErrorType::InvalidRequest);
    }

    /// Clause 5.5.4: every data-type violation is `BadRequestData`.
    #[test]
    fn every_data_type_violation_is_bad_request_data() {
        for refusal in [
            BatchBodyRefusal::NotAnArray {
                members: BatchMembers::Entities,
            },
            BatchBodyRefusal::Empty {
                members: BatchMembers::EntityIds,
            },
            BatchBodyRefusal::NullItem { index: 1 },
            BatchBodyRefusal::MissingId { index: 1 },
            BatchBodyRefusal::MalformedId {
                index: 1,
                source: UriParseError,
            },
            BatchBodyRefusal::NotAnEntityId { index: 1 },
        ] {
            pretty_assertions::assert_eq!(refusal.error_type(), NgsiLdErrorType::BadRequestData, "{refusal}");
        }
    }

    #[test]
    fn a_malformed_id_names_its_item_and_not_its_value() {
        let refusal = BatchBodyRefusal::MalformedId {
            index: 2,
            source: UriParseError,
        };
        pretty_assertions::assert_eq!(
            refusal.to_string(),
            "the 'id' of batch item 2 is not a URI: invalid URI: not a valid URN, URL or IRI"
        );
    }

    #[test]
    fn the_array_refusals_name_what_the_array_was_to_hold() {
        pretty_assertions::assert_eq!(
            BatchBodyRefusal::Empty {
                members: BatchMembers::EntityIds
            }
            .to_string(),
            "the request body must be a non-empty JSON array of entity ids"
        );
        pretty_assertions::assert_eq!(
            BatchBodyRefusal::NotAnArray {
                members: BatchMembers::Entities
            }
            .to_string(),
            "the request body must be a JSON array of entities"
        );
    }

    #[test]
    fn the_problem_carries_the_instance_the_status_and_the_detail() {
        let problem = BatchBodyRefusal::NullItem { index: 3 }.into_problem(Uri::from_static("/ngsi-ld/v1/entityOperations/upsert"));
        pretty_assertions::assert_eq!(problem.problem().error_type(), NgsiLdErrorType::BadRequestData);
        pretty_assertions::assert_eq!(problem.problem().status_code(), StatusCode::BAD_REQUEST);
        pretty_assertions::assert_eq!(problem.problem().detail(), "batch item 3 is null");
        pretty_assertions::assert_eq!(
            problem.problem().instance().map(ToString::to_string).as_deref(),
            Some("/ngsi-ld/v1/entityOperations/upsert")
        );
    }
}
