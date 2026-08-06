//! Rendering a schema identifier into a quoted SQL column reference.
//!
//! A column named by an [`Iden`] is turned into the `"col"` or `"table"."col"`
//! text a custom SQL fragment interpolates. A fragment that must name a column
//! inside `cust`/`cust_with_values` — where sea-query cannot render the column
//! for it — references it by the same iden the query builder uses, so a renamed
//! column moves the reference with it rather than leaving a hand-quoted string.

use sea_query::Iden;

/// Quote one identifier: `"name"`.
pub(crate) fn quoted(identifier: impl Iden) -> String {
    let name = identifier.to_string();
    format!("\"{name}\"")
}

/// Quote a `relation.column` reference: `"relation"."column"`.
pub(crate) fn qualified(relation: impl Iden, column: impl Iden) -> String {
    let relation = relation.to_string();
    let column = column.to_string();
    format!("\"{relation}\".\"{column}\"")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{E, Entities};

    #[test]
    fn one_identifier_is_double_quoted() {
        assert_eq!(quoted(Entities::Payload), "\"payload\"");
    }

    #[test]
    fn a_qualified_reference_quotes_both_halves() {
        assert_eq!(qualified(E, Entities::Id), "\"e\".\"id\"");
    }
}
