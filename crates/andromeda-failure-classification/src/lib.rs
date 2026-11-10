//! The client-safe NGSI-LD problem each broker failure is reported as.
//!
//! A failure of entity, temporal, query, parameter, JSON-LD, store or forwarding
//! policy reaches a client through more than one door: the HTTP binding answers
//! it, and a Snapshot records it in the execution result of the query that met
//! it (clause 5.2.42). Each such failure is classified here once, into its
//! clause 5.5.2 category or clause 6.3.17 binding, the detail a client may read,
//! and how soon a retryable one may be repeated, so every door reports it the
//! same way. Nothing here names a transport: attaching a request instance and
//! writing headers is the binding's.

pub mod classified_report;
pub mod entity_failure;
pub mod forwarding_failure;
pub mod internal_failure_summary;
pub mod jsonld_failure;
pub mod parameter_failure;
pub mod query_failure;
pub mod result_limit_failure;
pub mod store_failure;
pub mod temporal_failure;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [
        "andromeda-entity",
        "andromeda-federation",
        "andromeda-jsonld",
        "andromeda-ngsi-ld-problem-details",
        "andromeda-ngsi-ld-query",
        "andromeda-ngsi-ld-request",
        "andromeda-persistence-failure",
        "andromeda-registration",
        "andromeda-temporal",
    ],
    allow_workspace_dev_dependencies: [
        "andromeda-context-commit",
        "andromeda-http-headers",
        "andromeda-ngsi-ld-geometry",
        "andromeda-ngsi-ld-lexicon",
        "andromeda-ngsi-ld-number",
        "andromeda-ngsi-ld-representation",
    ],
    allow_production_dependencies: [],
}

#[cfg(test)]
andromeda_architecture_checks::forbid_sql!();
