//! PostgreSQL implementation of the atomic context-commit protocol.
//!
//! The crate owns the transaction that commits current entity state, native
//! PostgreSQL temporal history, notification outbox entries and durable
//! asynchronous side-effect jobs. PostgreSQL statement fusion and projection
//! publication remain private implementation details.

#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]

mod columns;
/// Native PostgreSQL implementation of the context commit port.
pub mod committer;
mod entity_group;
mod history;

#[cfg(test)]
mod boundary {
    /// The binding crate may depend on the policy port and the PostgreSQL
    /// persistence crates it composes, but never on the binary.
    #[test]
    fn this_crate_declares_only_what_its_layer_permits() {
        /// In-workspace crates this one may name in any dependency table.
        const ALLOWED_IN_WORKSPACE: &[&str] = &[
            "andromeda-context-commit",
            "andromeda-layer-guard",
            "andromeda-ngsi-ld-geometry",
            "andromeda-ngsi-ld-lexicon",
            "andromeda-postgresql-context-store",
            "andromeda-postgresql-query",
            "andromeda-postgresql-temporal-store",
        ];
        let forbidden = andromeda_layer_guard::manifest::baseline_forbidden_except(&["sqlx"]);
        andromeda_layer_guard::manifest::assert_layer(env!("CARGO_MANIFEST_DIR"), &forbidden, ALLOWED_IN_WORKSPACE);
    }
}
