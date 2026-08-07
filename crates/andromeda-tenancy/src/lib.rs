//! One broker, many tenants, and the runtime each of them gets.
//!
//! ETSI GS CIM 009 v1.9.1 clause 6.3.14 defines the `NGSILD-Tenant` header and
//! clause 4.14 says a tenant is an isolated scope: an entity created in one is
//! invisible from another. It says nothing about how a broker achieves that,
//! which is why nothing here is a reading of the specification —
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

pub mod config;
pub mod creation_policy;
pub mod database;
pub mod database_name;
pub mod db_name_prefix;
pub mod error;
pub mod runtime;
pub mod state_factory;
pub mod status;
pub mod tenant_ref;

#[cfg(test)]
mod boundary {
    /// The layer rule this crate is on the inside of.
    ///
    /// Asserted here rather than by a central test so the failure names the
    /// crate that broke the rule, and so a crate added to the workspace cannot
    /// be forgotten by a table it was never added to. The rule itself is read
    /// once, in `andromeda-layer-guard`, which every crate carries as a
    /// dev-dependency and which declares nothing of its own.
    ///
    /// `sqlx` heads the forbidden list for the reason this crate exists: a
    /// tenant registry that could issue its own query would make the eviction
    /// policy and the storage backend one thing again, and the whole point of
    /// [`crate::database::TenantDatabase`] is that they are not.
    #[test]
    fn this_crate_declares_only_what_its_layer_permits() {
        /// In-workspace crates this one may name, in any dependency table.
        /// Anything else is an upward or sideways edge.
        const ALLOWED_IN_WORKSPACE: &[&str] = &["andromeda-layer-guard", "andromeda-ngsi-ld-lexicon"];

        // Tenancy reads its registry configuration and single-flights cold
        // starts, so it ships `confique` and a `tokio` sync/time slice.
        let forbidden = andromeda_layer_guard::manifest::baseline_forbidden_except(&["confique", "tokio"]);
        andromeda_layer_guard::manifest::assert_layer(env!("CARGO_MANIFEST_DIR"), &forbidden, ALLOWED_IN_WORKSPACE);
    }
}

#[cfg(test)]
mod dialect {
    /// No storage dialect may be written here, in any form.
    ///
    /// The manifest guard above reads dependency names, which is exactly what a
    /// dialect emitted as a string constant slips past: a crate can spell an
    /// entire SQL predicate without declaring `sea-query`.
    #[test]
    fn no_source_file_in_this_crate_emits_a_storage_dialect() {
        andromeda_layer_guard::dialect::assert_no_dialect(env!("CARGO_MANIFEST_DIR"));
    }
}
