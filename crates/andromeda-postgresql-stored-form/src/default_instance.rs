//! Whether a stored grain key is the default instance's (clause 4.5.5).
//!
//! The default instance carries no `datasetId` and its grain key is the empty
//! string, which no identifier is. Both predicates write that key inline, so
//! a partial index predicate on the key column is provable.

use andromeda_postgresql_seaquery::literal::text_literal;
use andromeda_postgresql_stored_values::dataset_key::DatasetKey;
use sea_query::{ExprTrait, SimpleExpr};

/// Whether the grain key `column` is the default instance's, the empty string
/// written inline so a partial index predicate on it is provable.
#[must_use]
pub fn is_default_instance(column: impl Into<SimpleExpr>) -> SimpleExpr {
    column.into().eq(text_literal(DatasetKey::DEFAULT.as_str()))
}

/// Whether the grain key `column` is a named instance's.
#[must_use]
pub fn is_named_instance(column: impl Into<SimpleExpr>) -> SimpleExpr {
    column.into().ne(text_literal(DatasetKey::DEFAULT.as_str()))
}

#[cfg(test)]
mod tests {
    use crate::default_instance::{is_default_instance, is_named_instance};
    use andromeda_postgresql_seaquery::assert_statement_snapshot;
    use sea_query::{Alias, Expr, Query, SelectStatement, SimpleExpr};

    fn render(expression: SimpleExpr) -> SelectStatement {
        Query::select().expr(expression).to_owned()
    }

    #[test]
    fn the_default_and_named_tests_compare_the_key_with_the_inline_empty_string() {
        let column = || Expr::col((Alias::new("s"), Alias::new("dataset_key")));
        assert_statement_snapshot!(
            "dataset-key-the-default-and-named-tests-compare-the-key-with-the-inline-empty-string-1",
            render(is_default_instance(column()))
        );
        assert_statement_snapshot!(
            "dataset-key-the-default-and-named-tests-compare-the-key-with-the-inline-empty-string-2",
            render(is_named_instance(column()))
        );
    }
}
