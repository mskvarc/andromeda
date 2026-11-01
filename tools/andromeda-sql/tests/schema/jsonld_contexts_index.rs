//! The index of `jsonld_contexts`.
//!
//! Clause 5.13.1 has the broker invalidate cached contexts as they expire, so
//! the sweep and the read of the next expiry bound `expires_at` among the
//! cached rows alone. The list operation filters by kind while it walks the
//! primary key, which a table of few rows serves without an index of the kind.

use crate::repository::{PinError, index_definition, schema};
use andromeda_sql::declared_schema::index::DeclaredIndex;
use pretty_assertions::assert_eq;

#[test]
fn the_cached_context_expiry_is_indexed_among_cached_contexts_alone() -> Result<(), PinError> {
    let schema = schema()?;
    let mut indexes: Vec<&str> = schema
        .indexes()
        .iter()
        .filter(|index| index.table() == "jsonld_contexts")
        .map(DeclaredIndex::name)
        .collect();
    indexes.sort_unstable();
    let indexes = indexes;
    assert_eq!(indexes, ["idx_jsonld_contexts_cached_expires_at"]);
    let definition = index_definition(&schema, "idx_jsonld_contexts_cached_expires_at")?;
    assert!(definition.ends_with("on jsonld_contexts (expires_at) where kind = 1"), "{definition}");
    Ok(())
}
