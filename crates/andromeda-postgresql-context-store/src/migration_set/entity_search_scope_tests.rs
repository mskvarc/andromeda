//! The Scope columns of `entity_search` and their index, checked against the
//! embedded migrations.
//!
//! An Entity without a Scope stores NULL in both Scope columns, never an
//! empty array, and the Scope index holds the scoped Entities alone. Clause
//! 4.19 matches a `scopeQ` against an Entity's Scopes, so an Entity without
//! one is never a match, and the strict `scopeQ` test implies the index
//! predicate.

use crate::migration_set::declared_schema::DeclaredSchema;

/// `text` with every run of whitespace collapsed to one space.
fn normalized(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[test]
fn an_entity_without_a_scope_stores_no_empty_scope_array() {
    let schema = DeclaredSchema::of_every_migration();
    let constraint = schema
        .constraints
        .iter()
        .find(|constraint| constraint.name == "chk_entity_search_scope_spelling")
        .unwrap_or_else(|| panic!("no migration declares the Scope spelling check"));
    let definition = normalized(&constraint.definition);
    assert!(definition.contains("(scope_paths is null) = (verbatim_scope_paths is null)"), "{definition}");
    assert!(definition.contains("scope_paths is null or (cardinality(scope_paths) > 0 and"), "{definition}");
}

#[test]
fn the_scope_index_holds_the_scoped_entities_alone() {
    let schema = DeclaredSchema::of_every_migration();
    let index = schema
        .indexes
        .iter()
        .find(|index| index.name == "idx_entity_search_scope_paths")
        .unwrap_or_else(|| panic!("no migration declares the Scope index"));
    pretty_assertions::assert_eq!(
        normalized(&index.definition),
        "on entity_search using gist (scope_paths) where scope_paths is not null"
    );
}
