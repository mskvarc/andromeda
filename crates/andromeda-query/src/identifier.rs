//! The `id` query parameter: a comma-separated list of resource identifiers
//! (clause 4.5.5).

use crate::error::QueryError;
use andromeda_domain::uri::Uri;

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
            trimmed.parse::<Uri>().map_err(|_| QueryError::ParamIdInvalid(trimmed.to_string()))
        })
        .collect()
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
    fn an_empty_entry_in_the_list_is_rejected() {
        let error = parse_id_list(&["urn:ngsi-ld:Vehicle:1".to_string(), String::new()]).expect_err("must reject");
        assert!(matches!(error, QueryError::ParamIdListEmpty), "got {error:?}");
    }
}
