//! HTTP binding of the notification receiver: the endpoint a context source
//! notifies this broker at for the subscriptions it forwarded (clauses 5.8.1.4
//! and 5.8.6).

#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]

pub mod error;
mod handler;
pub mod routes;
pub mod state;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [
        "andromeda-http-extraction",
        "andromeda-http-headers",
        "andromeda-http-response",
        "andromeda-jsonld",
        "andromeda-ngsi-ld-lexicon",
        "andromeda-ngsi-ld-model",
        "andromeda-ngsi-ld-problem-details",
        "andromeda-subscription-forwarding",
    ],
    allow_workspace_dev_dependencies: [],
    allow_production_dependencies: ["axum"],
}

#[cfg(test)]
andromeda_architecture_checks::forbid_sql!();
