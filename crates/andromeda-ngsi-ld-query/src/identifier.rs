//! The `id` query parameter: a comma-separated list of resource identifiers
//! (clause 4.5.5).

use crate::error::QueryError;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;

/// Parse the comma-separated `id` parameter into the URIs it names.
///
/// Every NGSI-LD resource is identified by a URI (clause 4.6.2), so a value
/// that is not one cannot name anything the broker stores and the request is
/// rejected rather than compiled into a filter that matches nothing.
///
/// An empty list means no identifier filter, which is why the compiled queries
/// carry a `Vec` rather than an `Option<Vec>`: "absent" and "empty" select the
/// same rows, and having two spellings of that invited a `Some(ids) &&
/// !ids.is_empty()` guard at every use.
///
/// # Errors
///
/// Returns [`QueryError::ParamIdListEmpty`] when an entry is blank, and
/// [`QueryError::ParamIdInvalid`] when an entry is not a clause 4.6.2 URI.
pub fn parse_id_list(raw: &[String]) -> Result<Vec<Identifier>, QueryError> {
    raw.iter()
        .map(|entry| {
            let trimmed = entry.trim();
            if trimmed.is_empty() {
                return Err(QueryError::ParamIdListEmpty);
            }
            parse_id(trimmed)
        })
        .collect()
}

/// Parse a single resource identifier into the URI it names.
///
/// The one place [`QueryError::ParamIdInvalid`] is minted: a caller that reads a
/// single `id` — a path segment, a stored identifier read back — routes through
/// here rather than re-parsing and re-mapping the fault, so every caller
/// reports the same typed parse failure.
///
/// # Errors
///
/// Returns [`QueryError::ParamIdInvalid`] when `raw` is not a clause 4.6.2 URI.
pub fn parse_id(raw: &str) -> Result<Identifier, QueryError> {
    raw.parse::<Identifier>().map_err(QueryError::ParamIdInvalid)
}

#[cfg(test)]
mod tests {
    use crate::{
        error::QueryError,
        identifier::{parse_id, parse_id_list},
    };
    use andromeda_ngsi_ld_lexicon::{
        identifier::{Identifier, IdentifierParseError},
        identifier_length::IDENTIFIER_BYTE_LIMIT,
    };
    use pretty_assertions::assert_eq;

    #[test]
    fn an_empty_parameter_is_no_filter() {
        assert_eq!(parse_id_list(&[]).expect("parses"), [] as [Identifier; 0]);
    }

    #[test]
    fn entity_ids_are_uris() {
        let parsed = parse_id_list(&["urn:ngsi-ld:Vehicle:1".to_string(), "https://example.com/thing/1".to_string()]).expect("parses");
        let rendered: Vec<&str> = parsed.iter().map(AsRef::as_ref).collect();
        assert_eq!(rendered, vec!["urn:ngsi-ld:Vehicle:1", "https://example.com/thing/1"]);
    }

    #[test]
    fn a_value_that_is_not_a_uri_is_rejected() {
        let error = parse_id_list(&["not a uri".to_string()]).expect_err("must reject");
        assert!(matches!(error, QueryError::ParamIdInvalid { .. }), "got {error:?}");
    }

    #[test]
    fn a_single_id_parses_or_reports_the_parse_failure() {
        assert_eq!(parse_id("urn:ngsi-ld:Vehicle:1").expect("parses").as_ref(), "urn:ngsi-ld:Vehicle:1");
        // A caller that reads a single id gets the same error as the list parser.
        let error = parse_id("not a uri").expect_err("must reject");
        assert!(matches!(&error, QueryError::ParamIdInvalid(_)), "got {error:?}");
    }

    /// An `id` too long to be an identifier is invalid, and the error says why
    /// rather than calling it no URI at all.
    #[test]
    fn an_over_long_id_parameter_is_invalid_with_its_reason() {
        let raw = format!("urn:ngsi-ld:Vehicle:{}", "a".repeat(IDENTIFIER_BYTE_LIMIT));
        let error = parse_id(&raw).expect_err("must reject");
        assert!(matches!(&error, QueryError::ParamIdInvalid(IdentifierParseError::TooLong(_))), "got {error:?}");
        assert!(error.to_string().contains("bytes"), "{error}");
    }

    #[test]
    fn an_empty_entry_in_the_list_is_rejected() {
        let error = parse_id_list(&["urn:ngsi-ld:Vehicle:1".to_string(), String::new()]).expect_err("must reject");
        assert!(matches!(error, QueryError::ParamIdListEmpty), "got {error:?}");
    }
}
