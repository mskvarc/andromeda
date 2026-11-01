//! The index a bounded temporal window reads its candidates from.
//!
//! The window candidates are the identity rows whose activity horizon is at
//! or after the window's start, a range read the planner serves from a B-tree
//! of the horizon alone. Only an Evolution holding an attribute instance has
//! a horizon, so the index is partial on the horizon being set, which every
//! range comparison implies.

use crate::repository::{PinError, index_definition, schema};

#[test]
fn the_window_candidates_read_a_partial_index_of_the_horizon() -> Result<(), PinError> {
    let schema = schema()?;
    let definition = index_definition(&schema, "idx_entities_evolution_active_until")?;
    assert!(
        definition.ends_with("on entities (evolution_active_until) where evolution_active_until is not null"),
        "{definition}"
    );
    Ok(())
}
