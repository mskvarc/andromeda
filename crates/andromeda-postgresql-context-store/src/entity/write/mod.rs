//! The statements that write an entity's current state: its identity, search
//! and type membership rows, and one `current_attributes` row per attribute
//! instance.
//!
//! There is one statement per kind of change, and it serves any number of
//! entities: a single-entity write is a set of one, which is why no per-row
//! variant exists. Each statement returns the rows it affected, and the caller
//! turns that into a per-entity outcome by asking what the write assumed.
//!
//! Insert and update write the attribute instance rows in the same statement,
//! paired with their entity by identifier, and return the identifier of every
//! entity they wrote. Delete returns what a deletion notification needs: the
//! instance rows go with the identity row, and clause 5.6.6 leaves the
//! temporal evolution standing.
//!
//! Every statement also keeps the discovery census
//! ([`census`](crate::entity::census)) in step with the search rows it writes,
//! in the same statement, counting from the images its own search write
//! returned ([`search_image`]).
//!
//! No statement here reads. A read-modify-write takes its "before" snapshot
//! outside the transaction and states what it assumed in a precondition, which
//! the update statement enforces.
//!
//! Each statement carries its parallel arrays as bound values placed where the
//! statement reads them: arrays that may hold NULL elements through
//! `nullable_array`, and the arrays of `jsonb` and of the
//! `sub_attribute_tree` composite as external arguments, which the adapter
//! numbers after the statement's own values.

#[cfg(test)]
use std::collections::BTreeSet;

/// The batch delete.
mod delete;
/// Taking back the Temporal Evolution a create established.
mod evolution_withdrawal;
/// The batch insert.
mod insert;
/// The latest instant an Entity holds, which no write may precede.
mod latest_instant;
/// The images of an `entity_search` row a set-write hands on.
pub(crate) mod search_image;
/// The staging shape every set-write reads its arrays through.
pub(crate) mod staging;
/// The common table expressions the set-writes name.
pub(crate) mod statement_ctes;
/// The type-membership rows each set-write keeps in step with the search row.
pub(crate) mod type_membership;
/// The batch update.
mod update;
/// The batch update's identity row write.
mod update_anchor;
/// The version a guarded write compares and the count every write moves.
mod version_guard;

/// Every `$n` placeholder in `sql`, as numbers.
#[cfg(test)]
fn placeholders(sql: &str) -> BTreeSet<u32> {
    sql.match_indices('$')
        .filter_map(|(at, _)| {
            let digits: String = sql[at + 1..].chars().take_while(char::is_ascii_digit).collect();
            digits.parse().ok()
        })
        .collect()
}
