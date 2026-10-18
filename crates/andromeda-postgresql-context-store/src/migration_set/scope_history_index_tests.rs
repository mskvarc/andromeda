//! The indexes of `scope_history`, checked against the embedded migrations.
//!
//! Clause 4.18 makes a `scopeQ` match an overlap of validity intervals, so the
//! per-Entity Scope probe reads the Scope of one Entity through the
//! entity-led B-trees. An index over the whole table that the probe can reach
//! is ANDed into that lookup, so the one index read across Entities, the
//! candidate read's path index, is partial on a predicate the probe never
//! states, which is why the set is pinned here.

use crate::migration_set::declared_schema::{DeclaredSchema, IndexKind};
use pretty_assertions::assert_eq;

#[test]
fn scope_history_is_indexed_by_its_entity_and_its_live_scope_paths() {
    let schema = DeclaredSchema::of_every_migration();
    let mut indexes: Vec<&str> = schema
        .indexes
        .iter()
        .filter(|index| index.table == "scope_history" && index.kind != IndexKind::Statistics)
        .map(|index| index.name.as_str())
        .collect();
    indexes.sort_unstable();
    let indexes = indexes;
    assert_eq!(
        indexes,
        [
            "idx_scope_history_entity_observed_at",
            "idx_scope_history_entity_recorded_at",
            "idx_scope_history_live_scope_path"
        ]
    );
}

#[test]
fn the_scope_candidates_state_the_partial_predicate_the_index_holds() {
    let schema = DeclaredSchema::of_every_migration();
    let index = schema
        .indexes
        .iter()
        .find(|index| index.name == "idx_scope_history_live_scope_path")
        .unwrap_or_else(|| panic!("the live Scope path index"));
    let definition = index.definition.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(definition.ends_with("using gist (scope_path) where value_kind = 1"), "{definition}");
}
