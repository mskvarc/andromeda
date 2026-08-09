//! Centralized framework response construction for Andromeda HTTP bindings.
//!
//! This crate owns every Axum response adapter and HTTP wire projection, including extraction rejections, batch projection and temporal range metadata. It deliberately chooses no policy-error category: each route binding supplies an exhaustive classifier or an already classified report.

#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]

pub mod batch;
pub mod batch_projection;
pub mod classified_report;
pub mod collection;
pub mod content_range;
pub mod extraction_rejection;
pub mod headers;
pub mod pagination;
pub mod problem_response;
pub mod render;
pub mod single;
pub mod sonic_json;
pub mod tenant_state_rejection;

#[cfg(test)]
mod boundary {
    /// Assert that centralized response construction reaches no route, storage, API, or server crate.
    #[test]
    fn this_crate_declares_only_projection_input_dependencies() {
        const ALLOWED_IN_WORKSPACE: &[&str] = &[
            "andromeda-http-headers",
            "andromeda-jsonld",
            "andromeda-layer-guard",
            "andromeda-ngsi-ld-lexicon",
            "andromeda-ngsi-ld-problem-details",
            "andromeda-ngsi-ld-query",
            "andromeda-ngsi-ld-render",
            "andromeda-ngsi-ld-vocabulary",
        ];

        let forbidden = andromeda_layer_guard::manifest::baseline_forbidden_except(&["axum", "sonic-rs"]);
        andromeda_layer_guard::manifest::assert_layer(env!("CARGO_MANIFEST_DIR"), &forbidden, ALLOWED_IN_WORKSPACE);
    }
}

#[cfg(test)]
mod dialect {
    /// Assert that centralized response code emits no storage dialect.
    #[test]
    fn no_source_file_in_this_crate_emits_a_storage_dialect() {
        andromeda_layer_guard::dialect::assert_no_dialect(env!("CARGO_MANIFEST_DIR"));
    }
}
