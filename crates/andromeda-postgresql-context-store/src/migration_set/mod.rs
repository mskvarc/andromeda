//! The three embedded migration sets and the ledger each is recorded in.
//!
//! The schema is described by three migration directories:
//!
//! * `platform/`: the platform tables (tenant registry, maintenance state,
//!   forwarded-subscription routes, wake schedule), applied only to the platform
//!   database and recorded in [`platform::PLATFORM_LEDGER`].
//! * `tenant/`: tenant-global relations (dictionaries, contexts,
//!   registrations, snapshot control records, forwarding mappings) and the
//!   TimescaleDB conversion of the live history tables.
//! * `branch/`: the relations every context branch holds: the live branch in
//!   `public` and each Snapshot branch in its own schema.
//!
//! All three directories share one version space: every file has a version no
//! other file in any directory has, and a new file of any kind takes the next
//! free number. The numbers are what order the files that depend on each other
//! (a branch relation references the dictionaries a tenant file creates, and
//! the forwarding mappings reference branch relations), so the tenant migrator
//! runs the union of `tenant/` and `branch/` in version order in `public`,
//! recorded in [`tenant::TENANT_LEDGER`]. The platform set, which holds the
//! lowest versions, runs first on the platform database, recorded in
//! [`platform::PLATFORM_LEDGER`]. The branch migrator runs `branch/` alone
//! inside a Snapshot schema, recorded in that schema's own
//! [`branch::BRANCH_LEDGER`]. Because every set has a ledger of its own, none of
//! them ever sees another's rows, so missing-migration detection stays on
//! everywhere.

/// The branch migration set, which creates and upgrades a Snapshot schema.
pub mod branch;
/// The platform migration set.
pub mod platform;
/// The tenant migration set: `tenant/` together with `branch/`.
pub mod tenant;

/// The indexes of attribute history, read by stream alone.
#[cfg(test)]
mod attribute_history_index_tests;
/// The comment rules every migration's text keeps.
#[cfg(test)]
mod comment_tests;
/// The shape of `current_attributes`, its key functions and its
/// statement-level references.
#[cfg(test)]
mod current_attributes_tests;
/// The lexical reading of migration DDL text.
#[cfg(test)]
mod ddl_text;
/// The schema objects the embedded migrations declare, read from their text.
#[cfg(test)]
pub(crate) mod declared_schema;
/// The append-only rule of the dictionaries.
#[cfg(test)]
mod dictionary_append_only_tests;
/// The naming and storage conventions, checked against the declared schema.
#[cfg(test)]
mod naming_tests;
/// The indexes of Scope history, read by Entity alone.
#[cfg(test)]
mod scope_history_index_tests;
/// The byte bounds the migrations state, tied to the Rust constants.
#[cfg(test)]
mod storage_bound_tests;
/// The vacuum, BRIN and sequence-cache parameters of the hot relations.
#[cfg(test)]
mod storage_parameter_tests;
/// The temporal catalogue's attribute walk index.
#[cfg(test)]
mod temporal_catalog_tests;
