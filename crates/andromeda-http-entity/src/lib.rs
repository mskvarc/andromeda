//! NGSI-LD entity, entity-operation, type-discovery, and attribute-discovery HTTP binding.
//!
//! This crate owns the handlers, entity-specific extraction, focused tenant state, route inventory, batch projection, and exhaustive classification of the focused errors consumed by clauses 5.6.1 through 5.6.10.

#![deny(missing_docs)]

pub mod attribute;
mod entity;
pub mod entity_error;
mod entity_operations;
pub mod entity_type;
pub mod error;
pub mod provision_forwarding;
pub mod routes;
pub mod snapshot_scope;
pub mod state;

#[cfg(test)]
mod boundary {
    /// Assert that entity HTTP depends only on focused policies and shared HTTP/specification crates.
    #[test]
    fn this_crate_has_no_storage_or_lateral_route_dependency() {
        const ALLOWED_IN_WORKSPACE: &[&str] = &[
            "andromeda-entity",
            "andromeda-entity-map",
            "andromeda-federation",
            "andromeda-http-extraction",
            "andromeda-http-headers",
            "andromeda-http-response",
            "andromeda-jsonld",
            "andromeda-layer-guard",
            "andromeda-ngsi-ld-lexicon",
            "andromeda-ngsi-ld-model",
            "andromeda-ngsi-ld-problem-details",
            "andromeda-ngsi-ld-query",
            "andromeda-ngsi-ld-render",
            "andromeda-ngsi-ld-representation",
            "andromeda-ngsi-ld-request",
            "andromeda-tenancy",
        ];

        let forbidden = andromeda_layer_guard::manifest::baseline_forbidden_except(&["axum"]);
        andromeda_layer_guard::manifest::assert_layer(env!("CARGO_MANIFEST_DIR"), &forbidden, ALLOWED_IN_WORKSPACE);
    }
}

#[cfg(test)]
mod dialect {
    /// Assert that no entity HTTP source emits a storage dialect.
    #[test]
    fn no_source_file_in_this_crate_emits_a_storage_dialect() {
        andromeda_layer_guard::dialect::assert_no_dialect(env!("CARGO_MANIFEST_DIR"));
    }
}
