//! What the apex binary is allowed to name, written down as a deliberate choice.
//!
//! Every other crate carries a boundary test that fails if it names a crate its
//! layer does not permit. The binary is the apex: it assembles the whole broker,
//! so it may name every in-workspace crate and ship every third-party runtime,
//! transport and driver a lower layer is forbidden. That freedom is exactly why
//! it needs its own assertion — not to catch an upward edge (there is no layer
//! above it) but to write the allow-list down, so a crate added to the workspace
//! is a deliberate new line here rather than a silent new dependency.
//!
//! The storage-dialect guard the passive crates carry is deliberately *not*
//! applied here: the binary owns the persistence layer and legitimately spells
//! SQL, so a dialect scan of its sources would reject the one place dialect
//! belongs.

#[cfg(test)]
mod boundary {
    use andromeda_layer_guard::{
        architecture::assert_retired_architecture_invariants,
        domain_string::assert_domain_string_inventory_text,
        raw_constant::assert_no_raw_iri_or_media_constants,
    };

    /// Every in-workspace crate the binary is entitled to name, in any table.
    ///
    /// The apex may name them all — this is the list of what "all" is, so a new
    /// workspace crate the binary starts depending on is added here on purpose.
    const ALLOWED_IN_WORKSPACE: &[&str] = &[
        "andromeda-context-commit",
        "andromeda-context-registry",
        "andromeda-delivery",
        "andromeda-entity",
        "andromeda-entity-map",
        "andromeda-federation",
        "andromeda-health",
        "andromeda-http-entity",
        "andromeda-http-extraction",
        "andromeda-jsonld",
        "andromeda-layer-guard",
        "andromeda-ngsi-ld-geometry",
        "andromeda-http-headers",
        "andromeda-http-response",
        "andromeda-ngsi-ld-lexicon",
        "andromeda-ngsi-ld-model",
        "andromeda-ngsi-ld-problem-details",
        "andromeda-ngsi-ld-query",
        "andromeda-ngsi-ld-render",
        "andromeda-ngsi-ld-representation",
        "andromeda-ngsi-ld-request",
        "andromeda-ngsi-ld-vocabulary",
        "andromeda-postgresql-atomic-write",
        "andromeda-postgresql-context-query",
        "andromeda-postgresql-context-store",
        "andromeda-postgresql-encoding",
        "andromeda-postgresql-query",
        "andromeda-postgresql-temporal-query",
        "andromeda-postgresql-temporal-store",
        "andromeda-registration",
        "andromeda-spatial-index",
        "andromeda-subscription",
        "andromeda-tenancy",
        "andromeda-test-common",
    ];

    /// The apex may ship every third-party crate a lower layer is forbidden, so
    /// its denylist is empty; the invariant this pins is which in-workspace
    /// crates it names, not which runtimes it ships.
    #[test]
    fn the_binary_names_only_the_workspace_crates_it_declares_it_may() {
        andromeda_layer_guard::manifest::assert_layer(env!("CARGO_MANIFEST_DIR"), &[], ALLOWED_IN_WORKSPACE);
    }

    /// Production declarations use canonical domain types or an exact exemption.
    #[test]
    fn production_domain_strings_match_the_reviewed_inventory_exactly() {
        assert_domain_string_inventory_text(env!("CARGO_MANIFEST_DIR"), include_str!("../docs/domain-string-inventory.tsv"));
    }

    /// Production IRI and media constants are validated by their declared type.
    #[test]
    fn production_constants_do_not_encode_iris_or_media_types_as_strings() {
        assert_no_raw_iri_or_media_constants(env!("CARGO_MANIFEST_DIR"));
    }

    /// Retired facades, dumping grounds, and composition names stay absent.
    #[test]
    fn retired_architecture_surfaces_do_not_return() {
        assert_retired_architecture_invariants(env!("CARGO_MANIFEST_DIR"));
    }
}
