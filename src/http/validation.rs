//! Shared parsing and validation helpers for HTTP handlers.
//!
//! Functions in this module extract and validate common path/query parameters
//! used across multiple handler modules (entities, temporal, subscriptions,
//! context source registrations, snapshots, entity maps).

use crate::{core::error::AndromedaError, http::error::problem::NgsiLdError};
use andromeda_ngsi_ld_lexicon::{
    name::grammar::Name,
    ngsi_ld_urn::{EntityMapId, SnapshotId},
    uri::Uri as ResourceUri,
};
use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
use andromeda_ngsi_ld_request::query::entity::EntityQueryParameters;
use axum::http::Uri;
use urn_rs::Urn;

/// Parse and validate an entity ID as a URN.
///
/// Returns `BadRequestData` if the string is not a valid URN.
pub fn parse_entity_id(id: &str, uri: &Uri) -> Result<Urn, NgsiLdError> {
    id.parse::<Urn>()
        .map_err(|_| NgsiLdError::from(AndromedaError::entity_validation("Invalid entity id: not a valid URI").with_instance(uri)))
}

/// Parse an optional `datasetId` query parameter as the URI it must be.
///
/// Absent leaves the target instance unselected; a present value that is not a
/// valid URI is a clause 5.6 `BadRequestData`, since a `datasetId` is a URI
/// (clause 4.5.5).
pub fn parse_dataset_id(raw: Option<&str>, uri: &Uri) -> Result<Option<Urn>, NgsiLdError> {
    raw.map(|value| {
        value
            .parse::<Urn>()
            .map_err(|_| NgsiLdError::from(AndromedaError::malformed_request("Invalid datasetId: not a valid URI").with_instance(uri)))
    })
    .transpose()
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
            AndromedaError::entity_validation("Name must not be empty").with_instance(uri),
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
                AndromedaError::entity_validation(format!(
                    "Invalid prefixed name '{value}': both prefix and local name must conform to NGSI-LD name grammar (clause 4.6.2)"
                ))
                .with_instance(uri),
            ));
        }
        return Ok(());
    }
    // Bare name — validate against the Name grammar.
    Name::new(value).map_err(|_| {
        NgsiLdError::from(
            AndromedaError::entity_validation(format!("Invalid name '{value}': does not conform to NGSI-LD name grammar (clause 4.6.2)")).with_instance(uri),
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
/// Instance IDs must be a clause 4.6.2 URI — a URN or a URL — which the lexicon
/// [`ResourceUri`] admits and an empty string does not.
pub fn validate_instance_id(id: &str, uri: &Uri) -> Result<(), NgsiLdError> {
    if id.parse::<ResourceUri>().is_err() {
        return Err(NgsiLdError::from(
            AndromedaError::entity_validation("Invalid instance id: not a valid URI").with_instance(uri),
        ));
    }
    Ok(())
}

/// Validate a context source registration ID.
///
/// CSR IDs must be a clause 4.6.2 URI — a URN or a URL — which the one lexicon
/// [`ResourceUri`] parser admits without the caller branching on the scheme
/// itself. Returns `BadRequestData` if the string is neither.
pub fn parse_csr_id(id: &str, uri: &Uri) -> Result<(), NgsiLdError> {
    id.parse::<ResourceUri>()
        .map_err(|_| NgsiLdError::from(AndromedaError::entity_validation("Invalid registration id: not a valid URI").with_instance(uri)))?;
    Ok(())
}

/// Parse and validate a snapshot ID as a typed NGSI-LD URN.
pub fn parse_snapshot_id(snapshot_id: &str, _uri: &Uri) -> Result<SnapshotId, NgsiLdError> {
    let id = match snapshot_id.parse::<SnapshotId>() {
        Ok(value) => value,
        Err(error) => return Err(AndromedaError::from(error).into()),
    };
    Ok(id)
}

/// Parse and validate an entity map ID as a typed NGSI-LD URN.
///
/// Delegates to `parse_entity_map_urn` from the entity map service layer.
pub fn parse_entity_map_id(id: &str, uri: &Uri) -> Result<EntityMapId, NgsiLdError> {
    let parsed = crate::service::entity_map::parse_entity_map_urn(id)?;
    let _ = uri;
    Ok(parsed)
}

/// Parsed `options` parameter for subscription and CSR subscription list endpoints.
#[derive(Debug, Default, Clone, Copy)]
pub struct ListOptions {
    /// Whether `count` was present in the options string.
    pub count: bool,
    /// Whether `sysAttrs` was present in the options string.
    pub sys_attrs: SysAttrsInclusion,
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
                "sysAttrs" => parsed.sys_attrs = SysAttrsInclusion::Include,
                other => {
                    return Err(NgsiLdError::from(
                        AndromedaError::malformed_request(format!("Invalid options value: '{}'", other)).with_instance(uri),
                    ));
                }
            }
        }
    }
    Ok(parsed)
}

/// Reject an entity-map creation that would match everything.
///
/// Clause 5.14.4.4 requires at least one of `type`, `attrs`, `q`, `georel`,
/// `scopeQ` or `id`: an entity map is a stable page over a result set, and a
/// result set with no filter is every entity the broker holds.
///
/// # Errors
///
/// Returns an `EntityMapValidation` problem when the query names no filter.
pub fn validate_entity_map_has_filter(query: &EntityQueryParameters, uri: &Uri) -> Result<(), NgsiLdError> {
    let has_filter = query.entity_type.is_some()
        || query.id.is_some()
        || query.id_pattern.is_some()
        || query.attrs.is_some()
        || query.geo.0.is_some()
        || query.scope_q.is_some()
        || query.q.is_some();

    if has_filter {
        return Ok(());
    }

    Err(NgsiLdError::from(
        AndromedaError::entity_map_validation("At least one query filter must be provided (type, attrs, q, georel, scopeQ, or id)").with_instance(uri),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::Uri;

    fn test_uri() -> Uri {
        "/test".parse().expect("valid test URI")
    }

    // -- parse_entity_id --

    #[test]
    fn parse_entity_id_valid_urn() {
        let result = parse_entity_id("urn:ngsi-ld:Entity:001", &test_uri());
        assert!(result.is_ok());
        pretty_assertions::assert_eq!(result.expect("valid URN").to_string(), "urn:ngsi-ld:Entity:001");
    }

    #[test]
    fn parse_entity_id_invalid() {
        let result = parse_entity_id("not-a-urn", &test_uri());
        assert!(result.is_err());
    }

    // -- validate_instance_id / parse_csr_id (clause 4.6.2 URI) --

    #[test]
    fn an_instance_id_accepts_a_urn_and_a_url_and_rejects_a_bare_name() {
        assert!(validate_instance_id("urn:ngsi-ld:Attribute:instance:1", &test_uri()).is_ok());
        assert!(validate_instance_id("https://example.com/instance/1", &test_uri()).is_ok());
        assert!(validate_instance_id("", &test_uri()).is_err());
        assert!(validate_instance_id("not a uri", &test_uri()).is_err());
    }

    #[test]
    fn a_csr_id_admits_both_a_urn_and_a_url_through_one_parser() {
        assert!(parse_csr_id("urn:ngsi-ld:ContextSourceRegistration:1", &test_uri()).is_ok());
        assert!(parse_csr_id("https://example.com/csr/1", &test_uri()).is_ok());
        assert!(parse_csr_id("not a uri", &test_uri()).is_err());
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
        pretty_assertions::assert_eq!(opts.sys_attrs, SysAttrsInclusion::Strip);
    }

    #[test]
    fn parse_list_options_sys_attrs() {
        let opts = parse_list_options(Some("sysAttrs"), &test_uri()).expect("valid options");
        assert!(!opts.count);
        pretty_assertions::assert_eq!(opts.sys_attrs, SysAttrsInclusion::Include);
    }

    #[test]
    fn parse_list_options_combined() {
        let opts = parse_list_options(Some("count,sysAttrs"), &test_uri()).expect("valid options");
        assert!(opts.count);
        pretty_assertions::assert_eq!(opts.sys_attrs, SysAttrsInclusion::Include);
    }

    #[test]
    fn parse_list_options_invalid_returns_error() {
        assert!(parse_list_options(Some("badValue"), &test_uri()).is_err());
    }

    #[test]
    fn parse_list_options_none_returns_defaults() {
        let opts = parse_list_options(None, &test_uri()).expect("valid options");
        assert!(!opts.count);
        pretty_assertions::assert_eq!(opts.sys_attrs, SysAttrsInclusion::Strip);
    }
}
