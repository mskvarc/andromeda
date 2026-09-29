//! Explicit scope and ordering for the broker's inbound HTTP middleware.

#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]

pub mod config;
pub mod stack;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [
        "andromeda-configuration-scalars",
        "andromeda-http-extraction",
        "andromeda-http-headers",
        "andromeda-http-tenancy",
    ],
    allow_workspace_dev_dependencies: [],
    allow_production_dependencies: ["axum"],
}

#[cfg(test)]
andromeda_architecture_checks::forbid_sql!();
