//! Focused NGSI-LD temporal HTTP route binding.

#![deny(missing_docs)]

pub mod error;
pub mod extension_gate;
mod forwarded_evolution_content;
mod history_read;
mod join;
mod operation_query;
mod request_context_admission;
pub mod routes;
pub mod state;
mod temporal;
mod temporal_answer;
/// The expanded history of an entity, for tests.
#[cfg(test)]
#[expect(
    clippy::expect_used,
    clippy::panic,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod test_history;
mod written_evolution_admission;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [
        "andromeda-context-space",
        "andromeda-entity-map",
        "andromeda-extensions",
        "andromeda-federation",
        "andromeda-http-context-registry",
        "andromeda-http-extraction",
        "andromeda-http-headers",
        "andromeda-http-response",
        "andromeda-jsonld",
        "andromeda-linked-retrieval",
        "andromeda-ngsi-ld-expansion",
        "andromeda-ngsi-ld-lexicon",
        "andromeda-ngsi-ld-model",
        "andromeda-ngsi-ld-problem-details",
        "andromeda-ngsi-ld-query",
        "andromeda-ngsi-ld-render",
        "andromeda-ngsi-ld-representation",
        "andromeda-ngsi-ld-request",
        "andromeda-temporal",
        "andromeda-tenancy",
    ],
    allow_workspace_dev_dependencies: ["andromeda-context-commit", "andromeda-ngsi-ld-geometry", "andromeda-ngsi-ld-representation", "andromeda-persistence-failure"],
    allow_production_dependencies: ["axum"],
}

#[cfg(test)]
andromeda_architecture_checks::forbid_sql!();
