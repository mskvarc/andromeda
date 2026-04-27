//! HTTP query parameter types shared across NGSI-LD API handlers.
//!
//! This module consolidates query parameter deserialization structs that were
//! previously scattered across individual handler files, eliminating duplication
//! of both struct definitions and the case-insensitive boolean deserializer.

pub mod csource_subscription;
pub mod entity;
pub mod entity_operations;
pub mod jsonld_context;
pub mod protocol;
pub mod query;
pub mod subscription;

use serde::{Deserialize, Deserializer};

/// Deserializes an optional boolean query parameter in a case-insensitive manner.
///
/// Accepts `"true"`, `"TRUE"`, `"True"`, `"false"`, `"FALSE"`, `"False"`, etc.
/// Returns `None` when the parameter is absent, `Some(bool)` when present and
/// valid, or an error for any non-boolean string value.
pub fn deserialize_option_bool_case_insensitive<'de, D>(deserializer: D) -> Result<Option<bool>, D::Error>
where
    D: Deserializer<'de>,
{
    let s: Option<String> = Option::deserialize(deserializer)?;
    match s {
        None => Ok(None),
        Some(s) => match s.to_lowercase().as_str() {
            "true" => Ok(Some(true)),
            "false" => Ok(Some(false)),
            other => Err(serde::de::Error::custom(format!("invalid boolean value: '{other}'"))),
        },
    }
}

/// Query parameters for endpoints that accept no parameters.
///
/// Used by subscription creation, context source subscription creation, and
/// temporal entity creation endpoints. The `deny_unknown_fields` attribute
/// rejects any unexpected query parameters.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EmptyParams {}

/// Query parameters for endpoints that only accept an optional `local` flag.
///
/// Shared by entity creation, deletion, merge, update, type retrieval, and
/// attribute retrieval endpoints.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalOnlyParams {
    #[serde(default, deserialize_with = "deserialize_option_bool_case_insensitive")]
    pub local: Option<bool>,
}

/// Query parameters for discovery list endpoints (types and attributes).
///
/// Both the `/types` and `/attributes` list endpoints accept `details` (a
/// plain boolean defaulting to `false`) and an optional case-insensitive
/// `local` flag.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiscoveryListParams {
    #[serde(default)]
    pub details: bool,
    #[serde(default, deserialize_with = "deserialize_option_bool_case_insensitive")]
    pub local: Option<bool>,
}

/// Query parameters for attribute deletion (both entity and temporal).
///
/// Shared by `DELETE /entities/{id}/attrs/{attrId}` and
/// `DELETE /temporal/entities/{id}/attrs/{attrId}`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeleteAttrParams {
    #[serde(rename = "datasetId")]
    pub dataset_id: Option<String>,
    #[serde(rename = "deleteAll", default, deserialize_with = "deserialize_option_bool_case_insensitive")]
    pub delete_all: Option<bool>,
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]
    use super::*;

    #[derive(Debug, Deserialize)]
    struct BoolWrapper {
        #[serde(default, deserialize_with = "deserialize_option_bool_case_insensitive")]
        val: Option<bool>,
    }

    #[test]
    fn deserialize_option_bool_absent() {
        let result: BoolWrapper = serde_urlencoded::from_str("").unwrap();
        pretty_assertions::assert_eq!(result.val, None);
    }

    #[test]
    fn deserialize_option_bool_true_lowercase() {
        let result: BoolWrapper = serde_urlencoded::from_str("val=true").unwrap();
        pretty_assertions::assert_eq!(result.val, Some(true));
    }

    #[test]
    fn deserialize_option_bool_true_uppercase() {
        let result: BoolWrapper = serde_urlencoded::from_str("val=TRUE").unwrap();
        pretty_assertions::assert_eq!(result.val, Some(true));
    }

    #[test]
    fn deserialize_option_bool_true_mixed_case() {
        let result: BoolWrapper = serde_urlencoded::from_str("val=True").unwrap();
        pretty_assertions::assert_eq!(result.val, Some(true));
    }

    #[test]
    fn deserialize_option_bool_false_lowercase() {
        let result: BoolWrapper = serde_urlencoded::from_str("val=false").unwrap();
        pretty_assertions::assert_eq!(result.val, Some(false));
    }

    #[test]
    fn deserialize_option_bool_false_uppercase() {
        let result: BoolWrapper = serde_urlencoded::from_str("val=FALSE").unwrap();
        pretty_assertions::assert_eq!(result.val, Some(false));
    }

    #[test]
    fn deserialize_option_bool_rejects_yes() {
        let result = serde_urlencoded::from_str::<BoolWrapper>("val=yes");
        assert!(result.is_err());
    }

    #[test]
    fn deserialize_option_bool_rejects_one() {
        let result = serde_urlencoded::from_str::<BoolWrapper>("val=1");
        assert!(result.is_err());
    }

    #[test]
    fn deserialize_option_bool_rejects_empty_value() {
        let result = serde_urlencoded::from_str::<BoolWrapper>("val=");
        assert!(result.is_err());
    }

    #[test]
    fn empty_params_accepts_empty_query() {
        let result = serde_urlencoded::from_str::<EmptyParams>("");
        assert!(result.is_ok());
    }

    #[test]
    fn empty_params_rejects_unknown_fields() {
        let result = serde_urlencoded::from_str::<EmptyParams>("foo=bar");
        assert!(result.is_err());
    }

    #[test]
    fn local_only_params_absent() {
        let result: LocalOnlyParams = serde_urlencoded::from_str("").unwrap();
        pretty_assertions::assert_eq!(result.local, None);
    }

    #[test]
    fn local_only_params_true_lowercase() {
        let result: LocalOnlyParams = serde_urlencoded::from_str("local=true").unwrap();
        pretty_assertions::assert_eq!(result.local, Some(true));
    }

    #[test]
    fn local_only_params_true_uppercase() {
        let result: LocalOnlyParams = serde_urlencoded::from_str("local=TRUE").unwrap();
        pretty_assertions::assert_eq!(result.local, Some(true));
    }

    #[test]
    fn local_only_params_false() {
        let result: LocalOnlyParams = serde_urlencoded::from_str("local=false").unwrap();
        pretty_assertions::assert_eq!(result.local, Some(false));
    }

    #[test]
    fn local_only_params_rejects_invalid() {
        let result = serde_urlencoded::from_str::<LocalOnlyParams>("local=yes");
        assert!(result.is_err());
    }

    #[test]
    fn local_only_params_rejects_unknown_fields() {
        let result = serde_urlencoded::from_str::<LocalOnlyParams>("unknown=x");
        assert!(result.is_err());
    }

    #[test]
    fn discovery_list_params_defaults() {
        let result: DiscoveryListParams = serde_urlencoded::from_str("").unwrap();
        assert!(!result.details);
        pretty_assertions::assert_eq!(result.local, None);
    }

    #[test]
    fn discovery_list_params_details_true() {
        let result: DiscoveryListParams = serde_urlencoded::from_str("details=true").unwrap();
        assert!(result.details);
        pretty_assertions::assert_eq!(result.local, None);
    }

    #[test]
    fn discovery_list_params_both_fields() {
        let result: DiscoveryListParams = serde_urlencoded::from_str("details=true&local=false").unwrap();
        assert!(result.details);
        pretty_assertions::assert_eq!(result.local, Some(false));
    }

    #[test]
    fn delete_attr_params_defaults() {
        let result: DeleteAttrParams = serde_urlencoded::from_str("").unwrap();
        pretty_assertions::assert_eq!(result.dataset_id, None);
        pretty_assertions::assert_eq!(result.delete_all, None);
    }

    #[test]
    fn delete_attr_params_both_fields() {
        let result: DeleteAttrParams = serde_urlencoded::from_str("datasetId=urn:test&deleteAll=true").unwrap();
        pretty_assertions::assert_eq!(result.dataset_id.as_deref(), Some("urn:test"));
        pretty_assertions::assert_eq!(result.delete_all, Some(true));
    }

    #[test]
    fn delete_attr_params_delete_all_case_insensitive() {
        let result: DeleteAttrParams = serde_urlencoded::from_str("deleteAll=TRUE").unwrap();
        pretty_assertions::assert_eq!(result.dataset_id, None);
        pretty_assertions::assert_eq!(result.delete_all, Some(true));
    }
}
