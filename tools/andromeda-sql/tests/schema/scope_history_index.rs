//! The indexes of `scope_history`.
//!
//! Clause 4.18 makes a `scopeQ` match an overlap of validity intervals, so the
//! per-Entity Scope probe reads the Scope of one Entity through the
//! entity-led B-trees. An index over the whole table that the probe can reach
//! is intersected with that lookup, so the one index read across Entities, the
//! candidate read's path index, is partial on a predicate the probe never
//! states, which is why the set is pinned here.

use crate::repository::{PinError, index_definition, schema};
use andromeda_sql::declared_schema::index::DeclaredIndex;
use pretty_assertions::assert_eq;

#[test]
fn scope_history_is_indexed_by_its_entity_and_its_live_scope_paths() -> Result<(), PinError> {
    let schema = schema()?;
    let mut indexes: Vec<&str> = schema
        .indexes()
        .iter()
        .filter(|index| index.table() == "scope_history")
        .map(DeclaredIndex::name)
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
    Ok(())
}

#[test]
fn the_scope_candidates_state_the_partial_predicate_the_index_holds() -> Result<(), PinError> {
    let schema = schema()?;
    let definition = index_definition(&schema, "idx_scope_history_live_scope_path")?;
    assert!(definition.ends_with("using gist (scope_path) where value_kind = 1"), "{definition}");
    Ok(())
}
