//! Shared parsing and validation helpers for API handlers.
//!
//! Functions in this module extract and validate common path/query parameters
//! used across multiple handler modules (entities, temporal, subscriptions,
//! context source registrations).

use crate::{domain::name::Name, error::AndromedaError, http::error::problem::NgsiLdError};
use axum::http::{HeaderMap, Uri, header::CONTENT_TYPE};
use mediatype::{MediaTypeBuf, names};
use serde_json::Value;
use url::Url;
use urn::Urn;

/// Parse and validate an entity ID as a URN.
///
/// Returns `BadRequestData` if the string is not a valid URN.
pub fn parse_entity_id(id: &str, uri: &Uri) -> Result<Urn, NgsiLdError> {
    id.parse::<Urn>()
        .map_err(|_| NgsiLdError::from(AndromedaError::bad_request_data("Invalid entity id: not a valid URI").with_instance(uri.clone())))
}

/// Validate that a string is a valid NGSI-LD Name (clause 4.6.2) or a valid IRI.
///
/// Per ETSI GS CIM 009 v1.9.1 clause 4.6.2, Entity Type names, Property names,
/// and Relationship names must conform to the Name grammar when given as
/// short-hand strings: `name = unicodeLetter *nameChar` where `nameChar`
/// includes Unicode letters, digits, and underscore.
///
/// Names may also use the prefixed form `prefix:localName` (compact IRI), where
/// both prefix and local name individually conform to the Name grammar.
///
/// Full IRIs (containing `://`) and URNs (starting with `urn:`) pass through
/// without Name grammar validation.
pub fn validate_name_or_iri(value: &str, uri: &Uri) -> Result<(), NgsiLdError> {
    if value.is_empty() {
        return Err(NgsiLdError::from(
            AndromedaError::bad_request_data("Name must not be empty").with_instance(uri.clone()),
        ));
    }
    // Full IRI (HTTP, HTTPS, etc.) — pass through.
    if value.contains("://") {
        return Ok(());
    }
    // URN — pass through.
    if value.starts_with("urn:") {
        return Ok(());
    }
    // Prefixed name (compact IRI): prefix:localName.
    if let Some(colon_pos) = value.find(':') {
        let prefix = &value[..colon_pos];
        let local = &value[colon_pos + 1..];
        if Name::new(prefix).is_err() || Name::new(local).is_err() {
            return Err(NgsiLdError::from(
                AndromedaError::bad_request_data(format!(
                    "Invalid prefixed name '{value}': both prefix and local name must conform to NGSI-LD name grammar (clause 4.6.2)"
                ))
                .with_instance(uri.clone()),
            ));
        }
        return Ok(());
    }
    // Bare name — validate against the Name grammar.
    Name::new(value).map_err(|_| {
        NgsiLdError::from(
            AndromedaError::bad_request_data(format!("Invalid name '{value}': does not conform to NGSI-LD name grammar (clause 4.6.2)"))
                .with_instance(uri.clone()),
        )
    })?;
    Ok(())
}

/// Validate an attribute name path parameter.
///
/// Attribute names must be either a valid NGSI-LD Name per clause 4.6.2
/// (including the prefixed `prefix:localName` form) or a valid IRI.
pub fn validate_attr_name(name: &str, uri: &Uri) -> Result<(), NgsiLdError> {
    validate_name_or_iri(name, uri)
}

/// Validate an attribute instance ID path parameter.
///
/// Instance IDs must be valid URIs (parseable by `url::Url`).
pub fn validate_instance_id(id: &str, uri: &Uri) -> Result<(), NgsiLdError> {
    if id.is_empty() || Url::parse(id).is_err() {
        return Err(NgsiLdError::from(
            AndromedaError::bad_request_data("Invalid instance id: not a valid URI").with_instance(uri.clone()),
        ));
    }
    Ok(())
}

/// Validate a context source registration ID.
///
/// CSR IDs may be either URNs (starting with `urn:`) or URLs. Returns
/// `BadRequestData` if the string is neither.
pub fn parse_csr_id(id: &str, uri: &Uri) -> Result<(), NgsiLdError> {
    if id.starts_with("urn:") {
        id.parse::<Urn>()
            .map_err(|_| NgsiLdError::from(AndromedaError::bad_request_data("Invalid registration id: not a valid URI").with_instance(uri.clone())))?;
    } else {
        Url::parse(id)
            .map_err(|_| NgsiLdError::from(AndromedaError::bad_request_data("Invalid registration id: not a valid URI").with_instance(uri.clone())))?;
    }
    Ok(())
}

/// Parsed `options` parameter for subscription and CSR subscription list endpoints.
#[derive(Debug, Default, Clone, Copy)]
pub struct ListOptions {
    /// Whether `count` was present in the options string.
    pub count: bool,
    /// Whether `sysAttrs` was present in the options string.
    pub sys_attrs: bool,
}

/// Parse the `options` query parameter for subscription list endpoints.
///
/// Recognized values: `count`, `sysAttrs`. Returns `InvalidRequest` for
/// any unrecognized value.
pub fn parse_list_options(options: Option<&str>, uri: &Uri) -> Result<ListOptions, NgsiLdError> {
    let mut parsed = ListOptions::default();
    if let Some(opts) = options {
        for opt in opts.split(',').map(str::trim) {
            if opt.is_empty() {
                continue;
            }
            match opt {
                "count" => parsed.count = true,
                "sysAttrs" => parsed.sys_attrs = true,
                other => {
                    return Err(NgsiLdError::from(
                        AndromedaError::invalid_request(format!("Invalid options value: '{}'", other)).with_instance(uri.clone()),
                    ));
                }
            }
        }
    }
    Ok(parsed)
}

/// Check whether the `Content-Type` header indicates a JSON-compatible media type.
///
/// Returns `Ok(())` for `application/json`, `application/ld+json`,
/// `application/merge-patch+json`, or when the header is absent/unparseable
/// (permissive: we let the body parser decide). Returns `Err(())` only when
/// an explicitly non-JSON media type is present.
pub fn check_json_content_type(headers: &HeaderMap) -> Result<(), ()> {
    let Some(raw) = headers.get(CONTENT_TYPE).and_then(|v| v.to_str().ok()).filter(|s| !s.is_empty()) else {
        return Ok(());
    };
    let Ok(mt) = raw.parse::<MediaTypeBuf>() else {
        return Ok(());
    };
    if mt.ty() != names::APPLICATION {
        return Err(());
    }
    if mt.subty() == names::LD && mt.suffix() == Some(names::JSON) {
        return Ok(());
    }
    if mt.subty() == names::MERGE_PATCH && mt.suffix() == Some(names::JSON) {
        return Ok(());
    }
    if mt.subty() == names::JSON && mt.suffix().is_none() {
        return Ok(());
    }
    Err(())
}

/// Strict variant of content-type checking.
///
/// Returns `true` for `application/json`, `application/ld+json`, and
/// `application/merge-patch+json`. Returns `false` for missing, unparseable,
/// or non-JSON content types.
pub fn is_json_content_type(headers: &HeaderMap) -> bool {
    let raw = headers.get(CONTENT_TYPE).and_then(|v| v.to_str().ok()).unwrap_or("");
    let Ok(mt) = raw.parse::<MediaTypeBuf>() else {
        return false;
    };
    if mt.ty() != names::APPLICATION {
        return false;
    }
    if mt.subty() == names::LD && mt.suffix() == Some(names::JSON) {
        return true;
    }
    if mt.subty() == names::MERGE_PATCH && mt.suffix() == Some(names::JSON) {
        return true;
    }
    mt.subty() == names::JSON && mt.suffix().is_none()
}

/// Extract the `@context` value from a JSON-LD payload.
///
/// Returns `None` if the payload is not an object or has no `@context` key.
pub fn extract_body_context(payload: &Value) -> Option<Value> {
    payload.as_object().and_then(|obj| obj.get("@context")).cloned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::{HeaderMap, HeaderValue, Uri};

    fn test_uri() -> Uri {
        "/test".parse().expect("valid test URI")
    }

    // -- parse_entity_id --

    #[test]
    fn parse_entity_id_valid_urn() {
        let result = parse_entity_id("urn:ngsi-ld:Entity:001", &test_uri());
        assert!(result.is_ok());
        assert_eq!(result.expect("valid URN").to_string(), "urn:ngsi-ld:Entity:001");
    }

    #[test]
    fn parse_entity_id_invalid() {
        let result = parse_entity_id("not-a-urn", &test_uri());
        assert!(result.is_err());
    }

    // -- validate_name_or_iri --

    #[test]
    fn validate_name_or_iri_valid_bare_name() {
        assert!(validate_name_or_iri("temperature", &test_uri()).is_ok());
    }

    #[test]
    fn validate_name_or_iri_valid_name_with_underscore() {
        assert!(validate_name_or_iri("speed_limit", &test_uri()).is_ok());
    }

    #[test]
    fn validate_name_or_iri_valid_name_with_digits() {
        assert!(validate_name_or_iri("sensor42", &test_uri()).is_ok());
    }

    #[test]
    fn validate_name_or_iri_valid_unicode_name() {
        assert!(validate_name_or_iri("M\u{00fc}nchen", &test_uri()).is_ok());
    }

    #[test]
    fn validate_name_or_iri_valid_https_iri() {
        assert!(validate_name_or_iri("https://example.org/temperature", &test_uri()).is_ok());
    }

    #[test]
    fn validate_name_or_iri_valid_urn() {
        assert!(validate_name_or_iri("urn:ngsi-ld:default-context:temperature", &test_uri()).is_ok());
    }

    #[test]
    fn validate_name_or_iri_valid_prefixed_name() {
        assert!(validate_name_or_iri("ngsi:temperature", &test_uri()).is_ok());
    }

    #[test]
    fn validate_name_or_iri_invalid_empty() {
        assert!(validate_name_or_iri("", &test_uri()).is_err());
    }

    #[test]
    fn validate_name_or_iri_invalid_starts_with_digit() {
        assert!(validate_name_or_iri("42sensor", &test_uri()).is_err());
    }

    #[test]
    fn validate_name_or_iri_invalid_contains_hyphen() {
        assert!(validate_name_or_iri("speed-limit", &test_uri()).is_err());
    }

    #[test]
    fn validate_name_or_iri_invalid_contains_dot() {
        assert!(validate_name_or_iri("a.b", &test_uri()).is_err());
    }

    #[test]
    fn validate_name_or_iri_invalid_contains_space() {
        assert!(validate_name_or_iri("my name", &test_uri()).is_err());
    }

    #[test]
    fn validate_name_or_iri_invalid_prefixed_bad_prefix() {
        assert!(validate_name_or_iri("42bad:name", &test_uri()).is_err());
    }

    #[test]
    fn validate_name_or_iri_invalid_prefixed_bad_local() {
        assert!(validate_name_or_iri("ngsi:42bad", &test_uri()).is_err());
    }

    // -- validate_attr_name (delegates to validate_name_or_iri) --

    #[test]
    fn validate_attr_name_valid_simple() {
        assert!(validate_attr_name("temperature", &test_uri()).is_ok());
    }

    #[test]
    fn validate_attr_name_valid_uri() {
        assert!(validate_attr_name("https://example.org/temperature", &test_uri()).is_ok());
    }

    #[test]
    fn validate_attr_name_empty_returns_error() {
        assert!(validate_attr_name("", &test_uri()).is_err());
    }

    // -- validate_instance_id --

    #[test]
    fn validate_instance_id_valid() {
        assert!(validate_instance_id("urn:ngsi-ld:instance:001", &test_uri()).is_ok());
    }

    #[test]
    fn validate_instance_id_empty_returns_error() {
        assert!(validate_instance_id("", &test_uri()).is_err());
    }

    // -- parse_csr_id --

    #[test]
    fn parse_csr_id_valid_urn() {
        assert!(parse_csr_id("urn:ngsi-ld:ContextSourceRegistration:001", &test_uri()).is_ok());
    }

    #[test]
    fn parse_csr_id_valid_url() {
        assert!(parse_csr_id("https://example.org/csr/001", &test_uri()).is_ok());
    }

    #[test]
    fn parse_csr_id_invalid() {
        assert!(parse_csr_id("not a valid id", &test_uri()).is_err());
    }

    // -- parse_list_options --

    #[test]
    fn parse_list_options_count() {
        let opts = parse_list_options(Some("count"), &test_uri()).expect("valid options");
        assert!(opts.count);
        assert!(!opts.sys_attrs);
    }

    #[test]
    fn parse_list_options_sys_attrs() {
        let opts = parse_list_options(Some("sysAttrs"), &test_uri()).expect("valid options");
        assert!(!opts.count);
        assert!(opts.sys_attrs);
    }

    #[test]
    fn parse_list_options_combined() {
        let opts = parse_list_options(Some("count,sysAttrs"), &test_uri()).expect("valid options");
        assert!(opts.count);
        assert!(opts.sys_attrs);
    }

    #[test]
    fn parse_list_options_invalid_returns_error() {
        assert!(parse_list_options(Some("badValue"), &test_uri()).is_err());
    }

    #[test]
    fn parse_list_options_none_returns_defaults() {
        let opts = parse_list_options(None, &test_uri()).expect("valid options");
        assert!(!opts.count);
        assert!(!opts.sys_attrs);
    }

    // -- is_json_content_type --

    #[test]
    fn is_json_content_type_application_json() {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        assert!(is_json_content_type(&headers));
    }

    #[test]
    fn is_json_content_type_application_ld_json() {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/ld+json"));
        assert!(is_json_content_type(&headers));
    }

    #[test]
    fn is_json_content_type_merge_patch_json() {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/merge-patch+json"));
        assert!(is_json_content_type(&headers));
    }

    #[test]
    fn is_json_content_type_text_plain_returns_false() {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("text/plain"));
        assert!(!is_json_content_type(&headers));
    }

    #[test]
    fn is_json_content_type_missing_header_returns_false() {
        let headers = HeaderMap::new();
        assert!(!is_json_content_type(&headers));
    }

    // -- extract_body_context --

    #[test]
    fn extract_body_context_present() {
        let payload = serde_json::json!({"@context": "https://example.org/context.jsonld", "id": "urn:test"});
        let ctx = extract_body_context(&payload);
        assert_eq!(ctx, Some(serde_json::json!("https://example.org/context.jsonld")));
    }

    #[test]
    fn extract_body_context_absent() {
        let payload = serde_json::json!({"id": "urn:test"});
        assert!(extract_body_context(&payload).is_none());
    }

    #[test]
    fn extract_body_context_non_object() {
        let payload = serde_json::json!("just a string");
        assert!(extract_body_context(&payload).is_none());
    }

    // -- check_json_content_type --

    #[test]
    fn check_json_content_type_absent_header_is_ok() {
        let headers = HeaderMap::new();
        assert!(check_json_content_type(&headers).is_ok());
    }

    #[test]
    fn check_json_content_type_application_json_is_ok() {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        assert!(check_json_content_type(&headers).is_ok());
    }

    #[test]
    fn check_json_content_type_text_plain_is_err() {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("text/plain"));
        assert!(check_json_content_type(&headers).is_err());
    }
}
