//! The Scope columns of `entity_search` and their index.
//!
//! An Entity without a Scope stores NULL in both Scope columns, never an
//! empty array, and the Scope index holds the scoped Entities alone. Clause
//! 4.19 matches a `scopeQ` against an Entity's Scopes, so an Entity without
//! one is never a match, and the strict `scopeQ` test implies the index
//! predicate.

use crate::repository::{PinError, constraint_definition, index_definition, schema};

#[test]
fn an_entity_without_a_scope_stores_no_empty_scope_array() -> Result<(), PinError> {
    let schema = schema()?;
    let definition = constraint_definition(&schema, "chk_entity_search_scope_spelling")?;
    assert!(definition.contains("(scope_paths is null) = (verbatim_scope_paths is null)"), "{definition}");
    assert!(definition.contains("scope_paths is null or (cardinality(scope_paths) > 0 and"), "{definition}");
    Ok(())
}

#[test]
fn the_scope_index_holds_the_scoped_entities_alone() -> Result<(), PinError> {
    let schema = schema()?;
    pretty_assertions::assert_eq!(
        index_definition(&schema, "idx_entity_search_scope_paths")?,
        "on entity_search using gist (scope_paths) where scope_paths is not null"
    );
    Ok(())
}
