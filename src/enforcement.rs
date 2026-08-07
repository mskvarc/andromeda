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
    /// Every in-workspace crate the binary is entitled to name, in any table.
    ///
    /// The apex may name them all — this is the list of what "all" is, so a new
    /// workspace crate the binary starts depending on is added here on purpose.
    const ALLOWED_IN_WORKSPACE: &[&str] = &[
        "andromeda-delivery",
        "andromeda-health",
        "andromeda-jsonld",
        "andromeda-layer-guard",
        "andromeda-ngsi-ld-geometry",
        "andromeda-ngsi-ld-http-headers",
        "andromeda-ngsi-ld-lexicon",
        "andromeda-ngsi-ld-model",
        "andromeda-ngsi-ld-problem-details",
        "andromeda-ngsi-ld-query",
        "andromeda-ngsi-ld-render",
        "andromeda-ngsi-ld-representation",
        "andromeda-ngsi-ld-request",
        "andromeda-ngsi-ld-vocabulary",
        "andromeda-postgresql-context-query",
        "andromeda-postgresql-encoding",
        "andromeda-postgresql-query",
        "andromeda-postgresql-temporal-query",
        "andromeda-spatial-index",
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
}
