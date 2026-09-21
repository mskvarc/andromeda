//! Advertising the enabled extensions in the Context Source Identity response.
//!
//! Clause 5.15.1.4 lets the identity response "include additional
//! configurational data dependent on the specific Context Source
//! implementation", and Andromeda holds `contextSourceExtras` as raw JSON that
//! is never JSON-LD-expanded. Everything inside it is therefore already
//! implementation-specific by construction, so the enabled extensions are
//! announced under a plain `extensions` key rather than a full vendor IRI: the
//! extension-strategy.md 5.4 "never `ngsi-ld:`" rule guards terms that get
//! expanded against a context, which an extras key is not.

use crate::enabled::EnabledExtensions;
use serde_json::{Map, Value};

/// The `contextSourceExtras` key the enabled extension tokens are listed under.
///
/// Not an IRI: it lives inside the raw, implementation-specific extras object,
/// which already scopes it away from the ETSI vocabulary.
pub const EXTENSION_KEY: &str = "extensions";

/// The advertised token for the temporal delete-by-query extension.
const TEMPORAL_DELETE_BY_QUERY_TOKEN: &str = "temporal-delete-by-query";
/// The advertised token for the entity-Purge dataset-scope extension.
const PURGE_DATASET_SCOPE_TOKEN: &str = "purge-dataset-scope";
/// The advertised token for the grouped temporal aggregation extension.
const TEMPORAL_AGGREGATION_GROUPING_TOKEN: &str = "temporal-aggregation-grouping";
/// The advertised token for the tenant-management extension.
const TENANT_MANAGEMENT_TOKEN: &str = "tenant-management";

/// Merge the enabled extension tokens into the operator's `contextSourceExtras`.
///
/// The operator's own extras (`base`) are preserved untouched; the enabled
/// tokens are added as a JSON array under [`EXTENSION_KEY`]. When no extension
/// is enabled the key is absent entirely, so a conformant broker advertises
/// exactly what the operator configured and nothing else.
///
/// When `base` is a non-object JSON value the operator supplied verbatim, it is
/// returned unchanged: there is no object to merge a member into, and dropping
/// the operator's value to advertise a token would lose configured data.
#[must_use]
pub fn enabled_extension_extras(base: Option<Value>, enabled: EnabledExtensions) -> Option<Value> {
    let tokens = enabled_tokens(enabled);
    if tokens.is_empty() {
        return base;
    }
    let mut object = match base {
        None => Map::new(),
        Some(Value::Object(members)) => members,
        Some(other) => return Some(other),
    };
    object.insert(
        EXTENSION_KEY.to_string(),
        Value::Array(tokens.into_iter().map(|token| Value::String(token.to_string())).collect()),
    );
    Some(Value::Object(object))
}

/// The advertised tokens for the enabled extensions, in a stable order.
fn enabled_tokens(enabled: EnabledExtensions) -> Vec<&'static str> {
    let mut tokens = Vec::new();
    if enabled.temporal_delete_by_query_enabled() {
        tokens.push(TEMPORAL_DELETE_BY_QUERY_TOKEN);
    }
    if enabled.purge_dataset_scope_enabled() {
        tokens.push(PURGE_DATASET_SCOPE_TOKEN);
    }
    if enabled.temporal_aggregation_grouping_enabled() {
        tokens.push(TEMPORAL_AGGREGATION_GROUPING_TOKEN);
    }
    if enabled.tenant_management_enabled() {
        tokens.push(TENANT_MANAGEMENT_TOKEN);
    }
    tokens
}

#[cfg(test)]
mod tests {
    use crate::{
        advertisement::{
            EXTENSION_KEY,
            PURGE_DATASET_SCOPE_TOKEN,
            TEMPORAL_AGGREGATION_GROUPING_TOKEN,
            TEMPORAL_DELETE_BY_QUERY_TOKEN,
            TENANT_MANAGEMENT_TOKEN,
            enabled_extension_extras,
        },
        config::ExtensionsConfig,
        enabled::EnabledExtensions,
    };
    use andromeda_http_stack::config::feature_mode::HttpFeatureMode;
    use serde_json::{Value, json};

    fn both_enabled() -> EnabledExtensions {
        ExtensionsConfig {
            conformance_mode: HttpFeatureMode::Disabled,
            temporal_delete_by_query: HttpFeatureMode::Enabled,
            purge_dataset_scope: HttpFeatureMode::Enabled,
            temporal_aggregation_grouping: HttpFeatureMode::Enabled,
            tenant_management: HttpFeatureMode::Enabled,
            temporal_delete_by_query_max_matches: 100,
            temporal_aggregation_grouping_max_groups: 100,
        }
        .resolve()
    }

    #[test]
    fn both_tokens_are_advertised_when_both_extensions_are_on() {
        let extras = enabled_extension_extras(None, both_enabled()).expect("extras present");
        let tokens = extras.get(EXTENSION_KEY).and_then(Value::as_array).expect("token array");
        let rendered: Vec<&str> = tokens.iter().filter_map(Value::as_str).collect();
        assert!(rendered.contains(&TEMPORAL_DELETE_BY_QUERY_TOKEN), "{rendered:?}");
        assert!(rendered.contains(&PURGE_DATASET_SCOPE_TOKEN), "{rendered:?}");
        assert!(rendered.contains(&TEMPORAL_AGGREGATION_GROUPING_TOKEN), "{rendered:?}");
        assert!(rendered.contains(&TENANT_MANAGEMENT_TOKEN), "{rendered:?}");
    }

    #[test]
    fn tenant_management_is_advertised_only_when_it_is_enabled() {
        let config = ExtensionsConfig {
            tenant_management: HttpFeatureMode::Enabled,
            ..ExtensionsConfig::default()
        };
        let extras = enabled_extension_extras(None, config.resolve()).expect("extras present");
        let tokens = extras.get(EXTENSION_KEY).and_then(Value::as_array).expect("token array");
        let rendered: Vec<&str> = tokens.iter().filter_map(Value::as_str).collect();
        pretty_assertions::assert_eq!(rendered, vec![TENANT_MANAGEMENT_TOKEN]);

        // With the extension off, nothing about it is advertised.
        assert!(enabled_extension_extras(None, ExtensionsConfig::default().resolve()).is_none());
    }

    #[test]
    fn no_vendor_key_is_added_when_no_extension_is_on() {
        let enabled = ExtensionsConfig::default().resolve();
        assert!(enabled_extension_extras(None, enabled).is_none());
        // The operator's own extras survive when nothing is advertised.
        let base = json!({"node": 1});
        pretty_assertions::assert_eq!(enabled_extension_extras(Some(base.clone()), enabled), Some(base));
    }

    #[test]
    fn operator_extras_are_preserved_beside_the_advertised_tokens() {
        let base = json!({"node": 1});
        let extras = enabled_extension_extras(Some(base), both_enabled()).expect("extras present");
        pretty_assertions::assert_eq!(extras.get("node"), Some(&json!(1)));
        assert!(extras.get(EXTENSION_KEY).is_some());
    }

    /// Clause 5.4 of the extension strategy: no advertised key may squat the
    /// ETSI `ngsi-ld:` namespace.
    #[test]
    fn no_advertised_key_uses_the_ngsi_ld_namespace() {
        let extras = enabled_extension_extras(Some(json!({"ngsi-ld:vocab": true})), both_enabled()).expect("extras present");
        let vendor_keys: Vec<&String> = extras
            .as_object()
            .expect("object")
            .keys()
            .filter(|key| key.as_str() != "ngsi-ld:vocab")
            .collect();
        for key in vendor_keys {
            assert!(!key.starts_with("ngsi-ld:"), "{key}");
        }
        pretty_assertions::assert_eq!(EXTENSION_KEY.starts_with("ngsi-ld:"), false);
    }
}
