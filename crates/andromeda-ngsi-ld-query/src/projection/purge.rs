//! Purge modes of the delete-entities-by-query operation (clause 5.6.21).

use crate::error::QueryError;
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;

/// Purge mode for the DELETE entities-by-query operation (S5.6.21).
///
/// When `drop` or `keep` parameters are provided, the operation modifies
/// entity attributes instead of deleting entire entities.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PurgeMode {
    /// Delete entire matching entities (default behaviour).
    DeleteEntities,
    /// Remove the listed attributes from matching entities.
    Drop(Vec<AttributeName>),
    /// Retain only the listed attributes on matching entities, removing all others.
    Keep(Vec<AttributeName>),
}

/// Parse the comma-separated `drop` / `keep` parameter value.
///
/// Clause 5.6.21 names attributes, so a value outside the clause 4.6.2 name
/// grammar cannot name one and the request is rejected rather than silently
/// purging nothing.
///
/// # Errors
///
/// Returns [`QueryError::AttributeNameInvalid`] when an entry is outside the
/// clause 4.6.2 attribute-name grammar.
pub fn parse_attribute_list(raw: &str) -> Result<Vec<AttributeName>, QueryError> {
    raw.split(',')
        .map(|entry| {
            let trimmed = entry.trim();
            trimmed.parse().map_err(|source| QueryError::AttributeNameInvalid {
                value: trimmed.to_string(),
                source,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_comma_separated_list_yields_one_name_per_entry() {
        let parsed = parse_attribute_list("speed, colour").expect("parses");
        let rendered: Vec<&str> = parsed.iter().map(AttributeName::as_str).collect();
        assert_eq!(rendered, vec!["speed", "colour"]);
    }

    #[test]
    fn a_value_outside_the_attribute_name_grammar_is_rejected() {
        let error = parse_attribute_list("speed,bad name").expect_err("must reject");
        assert!(matches!(error, QueryError::AttributeNameInvalid { .. }), "got {error:?}");
    }
}
