//! Centralized Axum request extraction for Andromeda's HTTP bindings.
//!
//! Every `FromRequest` and `FromRequestParts` implementation belongs here, including endpoint-specific body and query extractors and generic focused tenant-state extraction. Extractors may decode policy input types but never invoke policy services or select operations.

#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]

pub mod accept;
pub mod classification;
pub mod context;
pub mod entity_batch;
pub mod entity_query;
pub mod json_body;
pub mod jsonld;
pub mod language;
pub mod prefer;
pub mod protocol;
pub mod query;
pub mod snapshot;
pub mod snapshot_purge;
pub mod tenant;
pub mod tenant_state;
pub mod typed_path;
pub mod via;

#[cfg(test)]
mod boundary {
    /// Assert that centralized extraction reaches no route, storage, API, or server crate.
    #[test]
    fn this_crate_declares_only_extraction_input_and_response_dependencies() {
        const ALLOWED_IN_WORKSPACE: &[&str] = &[
            "andromeda-http-headers",
            "andromeda-http-response",
            "andromeda-jsonld",
            "andromeda-layer-guard",
            "andromeda-ngsi-ld-lexicon",
            "andromeda-ngsi-ld-model",
            "andromeda-ngsi-ld-problem-details",
            "andromeda-ngsi-ld-query",
            "andromeda-ngsi-ld-render",
            "andromeda-ngsi-ld-request",
            "andromeda-ngsi-ld-vocabulary",
            "andromeda-tenancy",
        ];

        let forbidden = andromeda_layer_guard::manifest::baseline_forbidden_except(&["axum", "sonic-rs"]);
        andromeda_layer_guard::manifest::assert_layer(env!("CARGO_MANIFEST_DIR"), &forbidden, ALLOWED_IN_WORKSPACE);
    }
}

#[cfg(test)]
mod dialect {
    /// Assert that centralized extraction emits no storage dialect.
    #[test]
    fn no_source_file_in_this_crate_emits_a_storage_dialect() {
        andromeda_layer_guard::dialect::assert_no_dialect(env!("CARGO_MANIFEST_DIR"));
    }
}
