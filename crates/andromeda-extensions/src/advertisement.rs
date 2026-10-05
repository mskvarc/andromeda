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

use crate::enabled::{EnabledExtensions, Extension};
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
/// The advertised token for Linked Entity retrieval on the temporal operations.
///
/// A federating broker reads it off a Context Source's identity before it
/// forwards a temporal `join` there: a source that does not list it answers
/// the standard temporal operations only.
pub const TEMPORAL_LINKED_RETRIEVAL_TOKEN: &str = "temporal-linked-retrieval";
/// The advertised token for the tenant-management extension.
const TENANT_MANAGEMENT_TOKEN: &str = "tenant-management";
/// The advertised token for the cursor-pagination extension.
const CURSOR_PAGINATION_TOKEN: &str = "cursor-pagination";

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
        tokens.push(token(Extension::TemporalDeleteByQuery));
    }
    if enabled.purge_dataset_scope_enabled() {
        tokens.push(token(Extension::PurgeDatasetScope));
    }
    if enabled.temporal_aggregation_grouping_enabled() {
        tokens.push(token(Extension::TemporalAggregationGrouping));
    }
    if enabled.temporal_linked_retrieval_enabled() {
        tokens.push(token(Extension::TemporalLinkedRetrieval));
    }
    if enabled.tenant_management_enabled() {
        tokens.push(token(Extension::TenantManagement));
    }
    if enabled.cursor_pagination_enabled() {
        tokens.push(token(Extension::CursorPagination));
    }
    tokens
}

/// The token one extension is advertised under.
const fn token(extension: Extension) -> &'static str {
    match extension {
        Extension::TemporalDeleteByQuery => TEMPORAL_DELETE_BY_QUERY_TOKEN,
        Extension::PurgeDatasetScope => PURGE_DATASET_SCOPE_TOKEN,
        Extension::TemporalAggregationGrouping => TEMPORAL_AGGREGATION_GROUPING_TOKEN,
        Extension::TemporalLinkedRetrieval => TEMPORAL_LINKED_RETRIEVAL_TOKEN,
        Extension::TenantManagement => TENANT_MANAGEMENT_TOKEN,
        Extension::CursorPagination => CURSOR_PAGINATION_TOKEN,
    }
}

/// Whether a Context Source's `contextSourceExtras` (Table 5.2.40-1)
/// advertise `extension`, read the way [`enabled_extension_extras`] writes it.
///
/// Extras that are absent, not an object, or list the tokens under anything
/// but an array advertise nothing: the member is implementation-specific, and
/// a source that does not follow this spelling has not said it serves the
/// extension.
#[must_use]
pub fn advertises(extras: Option<&Value>, extension: Extension) -> bool {
    extras
        .and_then(|extras| extras.get(EXTENSION_KEY))
        .and_then(Value::as_array)
        .is_some_and(|tokens| tokens.iter().any(|advertised| advertised.as_str() == Some(token(extension))))
}

#[cfg(test)]
mod tests {
    use crate::{
        advertisement::{
            CURSOR_PAGINATION_TOKEN,
            EXTENSION_KEY,
            PURGE_DATASET_SCOPE_TOKEN,
            TEMPORAL_AGGREGATION_GROUPING_TOKEN,
            TEMPORAL_DELETE_BY_QUERY_TOKEN,
            TEMPORAL_LINKED_RETRIEVAL_TOKEN,
            TENANT_MANAGEMENT_TOKEN,
            advertises,
            enabled_extension_extras,
        },
        config::ExtensionsConfig,
        enabled::{EnabledExtensions, Extension},
    };
    use andromeda_http_stack::config::feature_mode::HttpFeatureMode;
    use serde_json::{Value, json};

    fn both_enabled() -> EnabledExtensions {
        ExtensionsConfig {
            conformance_mode: HttpFeatureMode::Disabled,
            temporal_delete_by_query: HttpFeatureMode::Enabled,
            purge_dataset_scope: HttpFeatureMode::Enabled,
            temporal_aggregation_grouping: HttpFeatureMode::Enabled,
            temporal_linked_retrieval: HttpFeatureMode::Enabled,
            tenant_management: HttpFeatureMode::Enabled,
            cursor_pagination: HttpFeatureMode::Enabled,
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
        assert!(rendered.contains(&TEMPORAL_LINKED_RETRIEVAL_TOKEN), "{rendered:?}");
        assert!(rendered.contains(&TENANT_MANAGEMENT_TOKEN), "{rendered:?}");
        assert!(rendered.contains(&CURSOR_PAGINATION_TOKEN), "{rendered:?}");
    }

    #[test]
    fn tenant_management_is_advertised_only_when_it_is_enabled() {
        let config = ExtensionsConfig {
            tenant_management: HttpFeatureMode::Enabled,
            cursor_pagination: HttpFeatureMode::Disabled,
            ..ExtensionsConfig::default()
        };
        let extras = enabled_extension_extras(None, config.resolve()).expect("extras present");
        let tokens = extras.get(EXTENSION_KEY).and_then(Value::as_array).expect("token array");
        let rendered: Vec<&str> = tokens.iter().filter_map(Value::as_str).collect();
        pretty_assertions::assert_eq!(rendered, vec![TENANT_MANAGEMENT_TOKEN]);

        // With the extension off, nothing about it is advertised.
        let neither = ExtensionsConfig {
            cursor_pagination: HttpFeatureMode::Disabled,
            ..ExtensionsConfig::default()
        };
        assert!(enabled_extension_extras(None, neither.resolve()).is_none());
    }

    #[test]
    fn temporal_linked_retrieval_is_advertised_only_when_enabled() {
        let config = ExtensionsConfig {
            temporal_linked_retrieval: HttpFeatureMode::Enabled,
            cursor_pagination: HttpFeatureMode::Disabled,
            ..ExtensionsConfig::default()
        };
        let extras = enabled_extension_extras(None, config.resolve()).expect("extras present");
        let tokens = extras.get(EXTENSION_KEY).and_then(Value::as_array).expect("token array");
        let rendered: Vec<&str> = tokens.iter().filter_map(Value::as_str).collect();
        pretty_assertions::assert_eq!(rendered, vec![TEMPORAL_LINKED_RETRIEVAL_TOKEN]);

        let default_tokens = enabled_extension_extras(None, ExtensionsConfig::default().resolve()).expect("extras present");
        let default_rendered: Vec<&str> = default_tokens
            .get(EXTENSION_KEY)
            .and_then(Value::as_array)
            .expect("token array")
            .iter()
            .filter_map(Value::as_str)
            .collect();
        assert!(!default_rendered.contains(&TEMPORAL_LINKED_RETRIEVAL_TOKEN), "{default_rendered:?}");
    }

    /// A default broker serves cursor pagination, and says so.
    #[test]
    fn the_advertisement_names_cursor_pagination_by_default() {
        let extras = enabled_extension_extras(None, ExtensionsConfig::default().resolve()).expect("extras present");
        let tokens = extras.get(EXTENSION_KEY).and_then(Value::as_array).expect("token array");
        let rendered: Vec<&str> = tokens.iter().filter_map(Value::as_str).collect();
        pretty_assertions::assert_eq!(rendered, vec![CURSOR_PAGINATION_TOKEN]);
    }

    #[test]
    fn no_vendor_key_is_added_when_no_extension_is_on() {
        let enabled = EnabledExtensions::none();
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

    /// A broker reads back exactly what another one advertises.
    #[test]
    fn an_advertisement_reads_back_as_the_enabled_extensions() {
        let extras = enabled_extension_extras(None, both_enabled());
        assert!(advertises(extras.as_ref(), Extension::TemporalLinkedRetrieval));
        let default_extras = enabled_extension_extras(None, ExtensionsConfig::default().resolve());
        assert!(!advertises(default_extras.as_ref(), Extension::TemporalLinkedRetrieval));
        assert!(advertises(default_extras.as_ref(), Extension::CursorPagination));
    }

    /// Extras spelled any other way advertise nothing.
    #[test]
    fn extras_in_another_spelling_advertise_nothing() {
        for extras in [
            None,
            Some(json!("temporal-linked-retrieval")),
            Some(json!({"extensions": "temporal-linked-retrieval"})),
            Some(json!({"other": ["temporal-linked-retrieval"]})),
        ] {
            assert!(!advertises(extras.as_ref(), Extension::TemporalLinkedRetrieval), "{extras:?}");
        }
    }
}
