//! Shared functions for applying NGSI-LD protocol extension headers to query parameters.
//!
//! These functions consolidate the duplicated inline logic that merges protocol
//! headers (`NGSILD-Response-Format`, `NGSILD-Dataset-Id`, `NGSILD-Join`, etc.)
//! into query parameter structs across entity, temporal, and batch query handlers.

use crate::{
    core::error::AndromedaError,
    http::{
        error::problem::NgsiLdError,
        extractor::protocol::NgsiLdProtocolHeaders,
        parameter::{
            entity::GetEntityParams,
            query::{entity::EntityQueryParameters, temporal::TemporalQueryParameters},
        },
    },
};
use axum::http::Uri;

/// Convert dataset ID header values into a comma-separated string.
pub fn dataset_header_to_csv(protocol: &NgsiLdProtocolHeaders) -> Option<String> {
    protocol
        .dataset_id
        .as_ref()
        .map(|dataset| dataset.as_slice().iter().map(ToString::to_string).collect::<Vec<_>>().join(","))
}

/// Apply protocol extension headers to single-entity GET parameters.
///
/// Merges `NGSILD-Response-Format`, `NGSILD-Dataset-Id`, `NGSILD-Join`, and
/// `NGSILD-Join-Level` headers into the query parameters when the corresponding
/// query parameter is absent.
pub fn apply_entity_get_extension_headers(params: &mut GetEntityParams, protocol: &NgsiLdProtocolHeaders, uri: &Uri) -> Result<(), NgsiLdError> {
    // Only apply header if neither format nor options specifies a format
    let query_has_format = params
        .format
        .as_deref()
        .is_some_and(|f| matches!(f, "normalized" | "keyValues" | "simplified" | "concise"))
        || params
            .options
            .as_deref()
            .is_some_and(|opts| opts.contains("normalized") || opts.contains("keyValues") || opts.contains("simplified") || opts.contains("concise"));
    if !query_has_format && let Some(response_format) = protocol.response_format {
        let format_value = response_format.as_query_value();
        if !matches!(format_value, "normalized" | "keyValues" | "simplified" | "concise") {
            return Err(NgsiLdError::from(
                AndromedaError::malformed_request(format!("Invalid format value: '{}'", format_value)).with_instance(&uri),
            ));
        }
        params.format = Some(format_value.to_string());
    }
    if params.dataset_id.is_none() {
        params.dataset_id = dataset_header_to_csv(protocol);
    }
    if params.join.is_none()
        && let Some(join) = protocol.join
    {
        params.join = Some(join.into_inner().to_string());
    }
    if params.join_level.is_none()
        && let Some(join_level) = protocol.join_level
    {
        params.join_level = Some(join_level.into_inner());
    }
    Ok(())
}

/// Apply protocol extension headers to entity query parameters.
///
/// Merges `NGSILD-Scope`, `NGSILD-Response-Format`, `NGSILD-Dataset-Id`,
/// `NGSILD-Join`, and `NGSILD-Join-Level` headers into the query parameters
/// when the corresponding query parameter is absent.
pub fn apply_entity_query_extension_headers(params: &mut EntityQueryParameters, protocol: &NgsiLdProtocolHeaders, uri: &Uri) -> Result<(), NgsiLdError> {
    if params.scope_q.is_none()
        && let Some(scope) = protocol.scope.as_ref()
    {
        params.scope_q = Some(scope.as_str().to_string());
    }
    // Only apply header if neither format nor options specifies a format
    let query_has_format = params
        .format
        .as_deref()
        .is_some_and(|f| matches!(f, "normalized" | "keyValues" | "simplified" | "concise"))
        || params
            .options
            .as_deref()
            .is_some_and(|opts| opts.contains("normalized") || opts.contains("keyValues") || opts.contains("simplified") || opts.contains("concise"));
    if !query_has_format && let Some(response_format) = protocol.response_format {
        let format_value = response_format.as_query_value();
        if !matches!(format_value, "normalized" | "keyValues" | "simplified" | "concise") {
            return Err(NgsiLdError::from(
                AndromedaError::malformed_request(format!("Invalid format value: '{}'", format_value)).with_instance(&uri),
            ));
        }
        params.format = Some(format_value.to_string());
    }
    if params.dataset_id.is_none() {
        params.dataset_id = dataset_header_to_csv(protocol);
    }
    if params.join.is_none()
        && let Some(join) = protocol.join
    {
        params.join = Some(join.into_inner().to_string());
    }
    if params.join_level.is_none()
        && let Some(join_level) = protocol.join_level
    {
        params.join_level = Some(join_level.into_inner());
    }
    Ok(())
}

/// Apply protocol extension headers to temporal query parameters.
///
/// Merges `NGSILD-Response-Format` and `NGSILD-Dataset-Id` headers into the
/// temporal query parameters when the corresponding query parameter is absent.
/// Validates that the format value is a valid temporal format.
pub fn apply_temporal_extension_headers(params: &mut TemporalQueryParameters, protocol: &NgsiLdProtocolHeaders, uri: &Uri) -> Result<(), NgsiLdError> {
    if params.format.is_none()
        && let Some(response_format) = protocol.response_format
    {
        let format_value = response_format.as_query_value();
        if !matches!(format_value, "temporalValues" | "aggregatedValues") {
            return Err(NgsiLdError::from(
                AndromedaError::malformed_request(format!("Invalid format value: '{}'", format_value)).with_instance(&uri),
            ));
        }
        params.format = Some(format_value.to_string());
    }
    if params.dataset_id.is_none() {
        params.dataset_id = dataset_header_to_csv(protocol);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]
    use super::*;
    use crate::http::extractor::{prefer::PreferHeader, snapshot::SnapshotHeader};

    #[test]
    fn dataset_header_to_csv_returns_none_when_absent() {
        let protocol = build_empty_protocol();
        assert!(dataset_header_to_csv(&protocol).is_none());
    }

    #[test]
    fn apply_entity_get_extension_headers_noop_when_params_already_set() {
        let protocol = build_empty_protocol();
        let uri: Uri = "/test".parse().unwrap();
        let mut params = GetEntityParams {
            attrs: None,
            options: None,
            format: Some("keyValues".to_string()),
            pick: None,
            omit: None,
            lang: None,
            dataset_id: Some("urn:ds:1".to_string()),
            geometry_property: None,
            local: None,
            join: Some("inline".to_string()),
            join_level: Some(2),
            contained_by: None,
            entity_map: None,
            verify: None,
        };
        apply_entity_get_extension_headers(&mut params, &protocol, &uri).unwrap();
        pretty_assertions::assert_eq!(params.format.as_deref(), Some("keyValues"));
        pretty_assertions::assert_eq!(params.dataset_id.as_deref(), Some("urn:ds:1"));
        pretty_assertions::assert_eq!(params.join.as_deref(), Some("inline"));
        pretty_assertions::assert_eq!(params.join_level, Some(2));
    }

    #[test]
    fn apply_temporal_extension_headers_noop_when_params_set() {
        let protocol = build_empty_protocol();
        let uri: Uri = "/test".parse().unwrap();
        let mut params: TemporalQueryParameters = serde_urlencoded::from_str("format=temporalValues").unwrap();
        apply_temporal_extension_headers(&mut params, &protocol, &uri).unwrap();
        pretty_assertions::assert_eq!(params.format.as_deref(), Some("temporalValues"));
    }

    fn build_empty_protocol() -> NgsiLdProtocolHeaders {
        NgsiLdProtocolHeaders {
            language_filter: None,
            prefer: PreferHeader::default(),
            snapshot: SnapshotHeader(None),
            incoming_via: None,
            entity_map: None,
            scope: None,
            response_format: None,
            dataset_id: None,
            join: None,
            join_level: None,
        }
    }
}
