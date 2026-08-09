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
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [
        "andromeda-architecture-checks",
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
        "andromeda-task-runtime",
        "andromeda-tenancy",
        "andromeda-test-common",
    ],
    allow_workspace_dev_dependencies: [
        "andromeda-test-common",
    ],
    allow_production_dependencies: all,
}

#[cfg(test)]
andromeda_architecture_checks::workspace_architecture! {
    domain_string_inventory: "docs/domain-string-inventory.tsv",
    no_raw_iri_or_media_constants,
    retired_architecture,
}
