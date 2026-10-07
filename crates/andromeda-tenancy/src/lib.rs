//! One broker, many tenants, and the runtime each of them gets.
//!
//! ETSI GS CIM 009 v1.9.1 clause 6.3.14 defines the `NGSILD-Tenant` header and
//! clause 4.14 says a tenant is an isolated scope: an entity created in one is
//! invisible from another. It says nothing about how a broker achieves that,
//! which is why nothing here is a reading of the specification:
//! [`tenant_ref::TenantRef`] is the scope, and everything else is Andromeda's
//! decision to give each scope its own database, its own workers and its own
//! caches, and to hold only the recently used ones open.
//!
//! Two ports point outward, and they are the two halves of "where does this
//! tenant live and what does it need". [`database::TenantDatabase`] answers the
//! first and is implemented by whatever holds the tenant registry;
//! [`state_factory::TenantStateFactory`] answers the second and is implemented
//! by the composition root, because building a tenant's runtime means naming
//! every service the broker has. Neither is named here, which is what keeps
//! this crate from being the composition root itself.

#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]

pub mod administration;
pub mod config;
pub mod context_source_alias;
pub mod creation_policy;
pub mod database;
pub mod database_name;
pub mod db_name_prefix;
pub mod error;
pub mod lifecycle;
pub mod pending_work;
pub mod residency;
pub mod runtime;
pub mod runtime_build_failure;
pub mod state_factory;
pub mod status;
pub mod tenant_listing;
pub mod tenant_page_start;
pub mod tenant_record;
pub mod tenant_ref;
pub mod tenant_usage;
pub mod tenant_view;
pub mod usage_probe;
pub mod wake_schedule;
pub mod waker;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [
        "andromeda-configuration-scalars",
        "andromeda-ngsi-ld-lexicon",
        "andromeda-persistence-failure",
        "andromeda-telemetry",
    ],
    allow_workspace_dev_dependencies: [
    ],
    allow_production_dependencies: ["tokio"],
}

#[cfg(test)]
andromeda_architecture_checks::forbid_sql!();
