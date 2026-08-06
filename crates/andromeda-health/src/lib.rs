//! What "healthy" means to this broker, and how it is reported.
//!
//! ETSI GS CIM 009 v1.9.1 defines no health resource, so nothing here is a
//! reading of the specification: `GET /health` is an Andromeda extension and
//! [`report::HealthCheckResponse`] is its own shape. What the specification
//! does contribute is the identifier — clause 4.6.2's URI grammar, through
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
pub mod indicator;
pub mod report;
pub mod status;

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
    /// The forbidden list is the sharp end here. An indicator that probed a
    /// database from inside this crate would make "healthy" mean whatever
    /// PostgreSQL says, and the port exists precisely so it does not: the
    /// probe lives with the pool, not with the word.
    #[test]
    fn this_crate_declares_only_what_its_layer_permits() {
        /// Third-party crates this layer must never ship.
        const FORBIDDEN: &[&str] = &["sqlx", "sea-query", "geozero", "bytes", "axum", "reqwest", "rumqttc", "clap"];
        /// In-workspace crates this one may name, in any dependency table.
        /// Anything else is an upward or sideways edge.
        const ALLOWED_IN_WORKSPACE: &[&str] = &["andromeda-layer-guard", "andromeda-ngsi-ld-lexicon"];

        andromeda_layer_guard::manifest::assert_layer(env!("CARGO_MANIFEST_DIR"), FORBIDDEN, ALLOWED_IN_WORKSPACE);
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
