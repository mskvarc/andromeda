//! Architecture rules for broker composition dependencies.
//!
//! Every other crate carries a boundary test that fails if it names a crate its
//! layer does not permit. Composition is the concrete apex below the lean binary,
//! so it may name the selected policy, transport, runtime, and adapter crates.
//! This allow-list keeps every new concrete capability selection deliberate.
//!
//! The storage-dialect guard the passive crates carry is deliberately *not*
//! applied here: composition owns persistence selection and legitimately spells
//! SQL, so a dialect scan of its sources would reject the one place dialect
//! belongs.

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [
        "andromeda-architecture-checks",
        "andromeda-component-report",
        "andromeda-context-commit",
        "andromeda-context-registry",
        "andromeda-delivery",
        "andromeda-entity",
        "andromeda-entity-map",
        "andromeda-federation",
        "andromeda-health",
        "andromeda-http-api",
        "andromeda-http-component-report",
        "andromeda-http-context-registry",
        "andromeda-http-entity",
        "andromeda-http-entity-map",
        "andromeda-http-extraction",
        "andromeda-http-health",
        "andromeda-http-headers",
        "andromeda-http-registration",
        "andromeda-http-response",
        "andromeda-http-server",
        "andromeda-http-snapshot",
        "andromeda-http-source-identity",
        "andromeda-http-stack",
        "andromeda-http-subscription",
        "andromeda-http-telemetry",
        "andromeda-http-temporal",
        "andromeda-http-tenancy",
        "andromeda-jsonld",
        "andromeda-ngsi-ld-geometry",
        "andromeda-ngsi-ld-lexicon",
        "andromeda-ngsi-ld-model",
        "andromeda-ngsi-ld-problem-details",
        "andromeda-ngsi-ld-query",
        "andromeda-ngsi-ld-render",
        "andromeda-ngsi-ld-representation",
        "andromeda-ngsi-ld-request",
        "andromeda-ngsi-ld-vocabulary",
        "andromeda-notification",
        "andromeda-postgresql-atomic-write",
        "andromeda-postgresql-name-dictionary",
        "andromeda-postgresql-context-query",
        "andromeda-postgresql-context-store",
        "andromeda-postgresql-encoding",
        "andromeda-postgresql-query",
        "andromeda-postgresql-maintenance",
        "andromeda-postgresql-temporal-query",
        "andromeda-postgresql-temporal-store",
        "andromeda-registration",
        "andromeda-snapshot",
        "andromeda-spatial-index",
        "andromeda-subscription",
        "andromeda-task-runtime",
        "andromeda-telemetry",
        "andromeda-temporal",
        "andromeda-tenancy",
    ],
    allow_workspace_dev_dependencies: [
        "andromeda-test-client",
        "andromeda-test-fixtures",
        "andromeda-test-harness",
        "andromeda-test-mocks",
    ],
    allow_production_dependencies: all,
}

#[cfg(test)]
andromeda_architecture_checks::workspace_architecture! {
    workspace_root: "../..",
    no_raw_iri_or_media_constants,
    no_raw_sql,
    retired_architecture,
}

#[cfg(test)]
mod tests {
    #[test]
    fn composition_is_the_only_api_to_server_handoff_owner() {
        let broker = include_str!("broker.rs");
        let binary = include_str!("../../../src/main.rs");
        assert!(broker.contains("HttpApi"));
        assert!(broker.contains("execution::serve"));
        assert!(!binary.contains("HttpApi"));
        assert!(!binary.contains("execution::serve"));
    }
}
