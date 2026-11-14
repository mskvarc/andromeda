//! What a stored row means in a PostgreSQL statement, stated once for the
//! statements that write it and the statements that read it.
//!
//! Each fragment here is one fact of the stored form: which history rows record
//! a deletion, whether a stored Entity or instance still stands, which side of
//! an identity row a statement serves, the activity horizon a write rounds to,
//! whether a Scope assertion restates the Scope in force, how a write moves the
//! type-membership relation and merges type lists, which history columns a
//! queued entry is recorded in, how a column of dictionary keys reads back as
//! names, and how the default instance's grain key is told apart. A writer and
//! a reader that disagreed on one of them would store one thing and read
//! another, so both render the same fragment.
//!
//! The values these fragments compare and the relation identifiers they name
//! live below this crate. The query languages, which read the stored form
//! through these fragments, are spelled above it.

pub mod default_instance;
pub mod entity_sides;
pub mod evolution_horizon;
pub mod expiry;
pub mod history_entry_values;
pub mod history_grain;
pub mod memberships;
pub mod names_in_key_order;
pub mod scope_assertion;
pub mod tombstone;
pub mod type_list_union;
pub mod type_membership_moves;

#[cfg(test)]
andromeda_architecture_checks::dependency_boundary! {
    allow_workspace_dependencies: [
        "andromeda-postgresql-schema",
        "andromeda-postgresql-seaquery",
        "andromeda-postgresql-stored-values",
    ],
    allow_workspace_dev_dependencies: ["andromeda-postgresql-seaquery"],
    allow_production_dependencies: ["sea-query"],
}
