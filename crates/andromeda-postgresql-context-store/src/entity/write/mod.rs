//! The statements that write the three current-state entity relations and the
//! type memberships beside them.
//!
//! There is one statement per kind of change, and it serves any number of
//! entities: a single-entity write is a set of one, which is why no per-row
//! variant exists. Each statement returns the rows it affected, and the caller
//! turns that into a per-entity outcome by asking what the write assumed.
//!
//! Insert and update return the surrogate key alongside the identifier, because
//! the projection of current attribute values is written against that key in
//! this same transaction and must not have to read the row back to find it.
//! Delete returns neither: the projection is removed by the single ownership
//! edge that cascades from `entities`, and clause 5.6.6 leaves the temporal
//! evolution standing.
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
//! Each statement's parallel arrays bind through `sqlx` rather than sea-query:
//! the geometry, expiry and expectation arrays carry NULLs (`Vec<Option<T>>`),
//! which `sea-query-sqlx` cannot represent, so every array is bound outside
//! sea-query in the marker order the statement builder emits. The statement
//! structure — every relation, column, cast, function and clause — is still
//! built by sea-query.

#[cfg(test)]
use std::collections::BTreeSet;

/// The batch delete.
mod delete;
/// The batch insert.
mod insert;
/// The stored document a set-write lands, staged apart from the projection.
mod payload_staging;
/// The images of an `entity_search` row a set-write hands on.
pub(crate) mod search_image;
/// The staging shape every set-write reads its arrays through.
pub(crate) mod staging;
/// The type-membership rows each set-write keeps in step with the search row.
pub(crate) mod type_membership;
/// The batch update.
mod update;

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
