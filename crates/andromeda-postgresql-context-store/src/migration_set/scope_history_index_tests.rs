//! The indexes of `scope_history`, checked against the embedded migrations.
//!
//! Clause 5.7.4.4 keeps the Entities whose Scope instances match a `scopeQ`
//! only after selecting them by their Attribute instances, and clause 4.18
//! makes the match an overlap of validity intervals, so every read of Scope
//! history is the Scope of one Entity. An index over the whole table is then
//! only ever ANDed into the lookup of that Entity's few rows, which is why the
//! set is pinned here.

use crate::migration_set::declared_schema::{DeclaredSchema, IndexKind};
use pretty_assertions::assert_eq;

#[test]
fn scope_history_is_indexed_by_its_entity_alone() {
    let schema = DeclaredSchema::of_every_migration();
    let mut indexes: Vec<&str> = schema
        .indexes
        .iter()
        .filter(|index| index.table == "scope_history" && index.kind != IndexKind::Statistics)
        .map(|index| index.name.as_str())
        .collect();
    indexes.sort_unstable();
    let indexes = indexes;
    assert_eq!(indexes, ["idx_scope_history_entity_observed_at", "idx_scope_history_entity_recorded_at"]);
}
