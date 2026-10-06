//! What "healthy" means to this broker, and how it is reported.
//!
//! ETSI GS CIM 009 v1.9.1 defines no health resource, so nothing here is a
//! reading of the specification: `GET /health` is an Andromeda extension and
//! [`report::HealthCheckResponse`] is its own shape. What the specification
//! does contribute is the identifier — clause 4.6.2's name grammar, through
//! [`andromeda_ngsi_ld_lexicon::uri::Uri`].
//!
//! The crate is a port and a payload and nothing else. [`indicator::HealthIndicator`]
//! is implemented by whichever subsystem can actually answer for itself — the
//! MQTT connection pool, the PostgreSQL pool — and [`aggregate::Health`] runs
//! them and takes the worst answer. Neither of those subsystems is named here,
//! which is the whole point: adding a component means implementing the trait
//! and adding a [`component::ComponentName`], not editing an aggregator.

#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]

pub mod aggregate;
pub mod component;
pub mod details;
pub mod indicator;
pub mod mqtt_details;
pub mod postgresql_details;
pub mod report;
pub mod status;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [
        "andromeda-ngsi-ld-lexicon",
    ],
    allow_workspace_dev_dependencies: [
    ],
    allow_production_dependencies: [],
}

#[cfg(test)]
andromeda_architecture_checks::forbid_sql!();
