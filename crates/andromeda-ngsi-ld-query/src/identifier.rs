//! The `id` query parameter: a comma-separated list of resource identifiers
//! (clause 4.5.5).

use crate::error::QueryError;
use andromeda_ngsi_ld_lexicon::uri::Uri;

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
pub fn parse_id_list(raw: &[String]) -> Result<Vec<Uri>, QueryError> {
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
/// here rather than re-parsing and re-mapping the fault, so the value the error
/// carries is the identifier itself, not a parser's message.
///
/// # Errors
///
/// Returns [`QueryError::ParamIdInvalid`] when `raw` is not a clause 4.6.2 URI.
pub fn parse_id(raw: &str) -> Result<Uri, QueryError> {
    raw.parse::<Uri>().map_err(|_| QueryError::ParamIdInvalid(raw.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_parameter_is_no_filter() {
        assert!(parse_id_list(&[]).expect("parses").is_empty());
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
        assert!(matches!(error, QueryError::ParamIdInvalid(_)), "got {error:?}");
    }

    #[test]
    fn a_single_id_parses_or_reports_its_own_value_as_invalid() {
        assert_eq!(parse_id("urn:ngsi-ld:Vehicle:1").expect("parses").as_ref(), "urn:ngsi-ld:Vehicle:1");
        // The fault carries the identifier itself, not a parser's message, so a
        // caller that reads a single id gets the same error as the list parser.
        let error = parse_id("not a uri").expect_err("must reject");
        assert!(matches!(&error, QueryError::ParamIdInvalid(value) if value == "not a uri"), "got {error:?}");
    }

    #[test]
    fn an_empty_entry_in_the_list_is_rejected() {
        let error = parse_id_list(&["urn:ngsi-ld:Vehicle:1".to_string(), String::new()]).expect_err("must reject");
        assert!(matches!(error, QueryError::ParamIdListEmpty), "got {error:?}");
    }
}
