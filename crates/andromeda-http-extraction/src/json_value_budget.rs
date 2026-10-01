//! Refusing a JSON payload that holds more values than the broker accepts,
//! before it is parsed.
//!
//! ETSI GS CIM 009 v1.9.1 clause 6.3.2 raises "Request Entity too large" (413)
//! for an input "too large i.e. too many bytes", and IETF RFC 9110 section
//! 15.5.14 (the successor of the RFC 7231 section 6.5.11 the specification
//! references) defines the status as a refusal "because the content is larger
//! than the server is willing or able to process". A payload's processing cost
//! follows its number of JSON values far more than its bytes, so a payload
//! holding more values than the limit is refused with the same status, type
//! and advice as one holding more bytes.
//!
//! The census runs over the bytes the payload reader already holds, so unlike
//! the byte limit it leaves nothing unread on the connection and needs no
//! `Connection: close`.

use crate::{request_json_value_limit::RequestJsonValueLimit, request_payload::SPLIT_ADVICE};
use andromeda_http_response::classified_report::ClassifiedReport;
use andromeda_ngsi_ld_number::json_value::value_census::{JsonValueLimit, ValueCensusError, count_values_within};
use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
use thiserror::Error;

/// A payload holding more JSON values than the request limit admits.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
#[error("the request payload holds more than the {limit} JSON values this broker accepts in one request; {SPLIT_ADVICE}")]
pub struct RequestJsonValueBudgetExceeded {
    /// The limit the payload exceeds.
    limit: JsonValueLimit,
}

impl RequestJsonValueBudgetExceeded {
    /// The clause 6.3.2 "Request Entity too large" report of this refusal.
    #[must_use]
    pub fn report(self) -> ClassifiedReport {
        ClassifiedReport::public(NgsiLdErrorType::RequestEntityTooLarge, self.to_string())
    }
}

/// Refuse a JSON payload holding more values than `limit` admits.
///
/// Without a published limit nothing is counted. A payload that is not JSON
/// passes: the parse that follows refuses it with the wording its operation
/// reports a malformed payload in.
///
/// # Errors
///
/// Returns [`RequestJsonValueBudgetExceeded`] for a payload holding more JSON
/// values than `limit` admits.
pub fn refuse_values_past_budget(payload: &[u8], limit: Option<RequestJsonValueLimit>) -> Result<(), RequestJsonValueBudgetExceeded> {
    let Some(limit) = limit else {
        return Ok(());
    };
    match count_values_within(payload, limit.limit()) {
        Ok(_) | Err(ValueCensusError::Syntax(_)) => Ok(()),
        Err(ValueCensusError::OverLimit { limit }) => Err(RequestJsonValueBudgetExceeded { limit }),
    }
}

#[cfg(test)]
mod tests {
    use crate::{json_value_budget::refuse_values_past_budget, request_json_value_limit::RequestJsonValueLimit};
    use andromeda_ngsi_ld_number::json_value::value_census::JsonValueLimit;
    use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
    use std::num::NonZeroUsize;

    /// A published limit of `values`.
    fn limit(values: usize) -> Option<RequestJsonValueLimit> {
        NonZeroUsize::new(values).map(|values| RequestJsonValueLimit::new(JsonValueLimit::new(values)))
    }

    #[test]
    fn a_payload_within_the_limit_passes() {
        assert!(refuse_values_past_budget(br#"{"n":[1,2]}"#, limit(4)).is_ok());
    }

    #[test]
    fn a_payload_past_the_limit_is_a_request_entity_too_large_naming_the_limit() {
        let refused = refuse_values_past_budget(br#"{"id":"urn:ngsi-ld:T:1","type":"T","n":[1,2]}"#, limit(3)).expect_err("six values exceed three");
        let report = refused.report();
        pretty_assertions::assert_eq!(report.error_type(), NgsiLdErrorType::RequestEntityTooLarge);
        pretty_assertions::assert_eq!(
            refused.to_string(),
            "the request payload holds more than the 3 JSON values this broker accepts in one request; send the content in smaller requests, a batch operation as several smaller batches"
        );
    }

    #[test]
    fn a_payload_without_a_published_limit_is_not_counted() {
        assert!(refuse_values_past_budget(b"[1,2,3,4,5,6,7,8,9]", None).is_ok());
    }

    /// The parse that follows owns the refusal of a malformed payload.
    #[test]
    fn a_malformed_payload_is_left_to_the_parse() {
        assert!(refuse_values_past_budget(b"[1,", limit(10)).is_ok());
    }
}
