//! Decoding a followed pointer that names a posted query (clause 5.5.9).
//!
//! The query-string binding of a query answers both the query a client wrote
//! and the pointers of a query a client sent by POST, which name the query the
//! broker holds by its digest. A parameter beside the digest that the held query
//! already fixes is refused with `BadRequestData` before anything is decoded,
//! since the pointer was not followed as the broker built it.

use crate::{addressed_uri::addressed_uri, query_decoding::decode_query};
use andromeda_failure_classification::classified_report::ClassifiedReport;
use andromeda_http_response::{extraction_rejection::ExtractorRejection, problem_response::ProblemResponse, request_refusal::RequestRefusal};
use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
use andromeda_ngsi_ld_request::{
    pagination::posted_reference::{PostedQueryReference, split_posted_query},
    query_parameter::QueryParameterSet,
};
use axum::http::request::Parts;
use serde::de::DeserializeOwned;

/// Decode the query string of `parts` as a pointer naming a posted query,
/// beside the URL parameters `P` of the binding that posted it.
///
/// # Errors
///
/// A `BadRequestData` naming a parameter the posted query fixes or a digest that
/// is not one the broker emits, and the refusal of URL parameters that do not
/// decode.
pub(crate) fn decode_posted_reference<P>(parts: &Parts) -> Result<PostedQueryReference<P>, ExtractorRejection>
where
    P: DeserializeOwned + QueryParameterSet,
{
    let query = parts.uri.query().unwrap_or("");
    PostedQueryReference::<P>::refuse_reselection(query).map_err(|refused| bad_request_data(&refused, parts))?;
    let (digest, url) = split_posted_query(query).map_err(|refused| bad_request_data(&refused, parts))?;
    let url = decode_query::<P>(&url).map_err(|refusal| refusal.into_rejection(addressed_uri(parts)))?;
    Ok(PostedQueryReference { digest, url })
}

/// The `BadRequestData` a pointer not followed as the broker built it is
/// refused with.
fn bad_request_data(refused: &impl ToString, parts: &Parts) -> ExtractorRejection {
    ExtractorRejection::Classified(ProblemResponse::classified(
        ClassifiedReport::public(NgsiLdErrorType::BadRequestData, refused.to_string()),
        addressed_uri(parts),
    ))
}

/// Whether the query string of `parts` names a posted query.
pub(crate) fn names_posted_query<P: QueryParameterSet>(parts: &Parts) -> bool {
    PostedQueryReference::<P>::is_named_in(parts.uri.query().unwrap_or(""))
}

#[cfg(test)]
mod tests {
    use crate::posted_reference::decode_posted_reference;
    use andromeda_ngsi_ld_problem_details::{error_type::NgsiLdErrorType, problem_detail::ProblemDetail};
    use andromeda_ngsi_ld_request::{entity_operations::PostQueryParams, pagination::query_digest::QueryDigest};
    use axum::http::Request;
    use pretty_assertions::assert_eq;

    fn refusal(query: &str) -> Option<NgsiLdErrorType> {
        let (parts, ()) = Request::builder()
            .uri(format!("/ngsi-ld/v1/entities?{query}"))
            .body(())
            .expect("a valid request")
            .into_parts();
        decode_posted_reference::<PostQueryParams>(&parts)
            .err()
            .and_then(|rejection| rejection.problem().map(ProblemDetail::error_type))
    }

    #[test]
    fn a_pointer_as_the_broker_built_it_decodes() {
        let digest = QueryDigest::of(Some("type=Vehicle")).encode();
        assert_eq!(refusal(&format!("limit=2&offset=2&postedQuery={digest}")), None);
    }

    #[test]
    fn a_selection_beside_the_digest_is_bad_request_data() {
        let digest = QueryDigest::of(Some("type=Vehicle")).encode();
        assert_eq!(refusal(&format!("postedQuery={digest}&q=speed%3E1")), Some(NgsiLdErrorType::BadRequestData));
    }

    #[test]
    fn a_key_that_is_no_digest_is_bad_request_data() {
        assert_eq!(refusal("postedQuery=abc"), Some(NgsiLdErrorType::BadRequestData));
    }

    #[test]
    fn a_name_outside_the_vocabulary_is_invalid_request() {
        let digest = QueryDigest::of(Some("type=Vehicle")).encode();
        assert_eq!(refusal(&format!("postedQuery={digest}&bogus=1")), Some(NgsiLdErrorType::InvalidRequest));
    }
}
