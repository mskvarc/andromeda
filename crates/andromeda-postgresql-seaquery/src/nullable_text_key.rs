//! A nullable text column as a member of a unique key: `COALESCE(column, '')`.
//!
//! A key whose text member may be NULL cannot be compared with `=`, and the
//! null-aware `IS NOT DISTINCT FROM` is neither hashable, mergeable nor
//! indexable: PostgreSQL joins it by testing every pair of rows, so matching
//! `n` keys costs `n * n` comparisons. Keyed instead by the text with NULL read
//! as the empty string, the member compares with plain `=`, which a hash join
//! and an index scan both use. A unique index declared on the same expression
//! enforces the key and serves every comparison spelled through here.
//!
//! The empty string must be no value the column itself may hold; the relation
//! that keys a column this way refuses it with a check constraint. The empty
//! string is written inline: a bound parameter would leave the expression no
//! longer the one the index is declared on.

use crate::literal::text_literal;
use sea_query::{ExprTrait, Func, SimpleExpr};

/// `COALESCE(value, '')`: `value` as the key member a NULL takes part in as
/// the empty string.
#[must_use]
pub fn nullable_text_key(value: impl Into<SimpleExpr>) -> SimpleExpr {
    Func::coalesce([value.into(), text_literal("")]).into()
}

/// Whether `left` and `right` are the same key member, NULL matching NULL.
#[must_use]
pub fn same_nullable_text_key(left: impl Into<SimpleExpr>, right: impl Into<SimpleExpr>) -> SimpleExpr {
    nullable_text_key(left).eq(nullable_text_key(right))
}

/// Whether `value` is NULL, tested as its key member so the key's index
/// answers it.
#[must_use]
pub fn is_null_text_key(value: impl Into<SimpleExpr>) -> SimpleExpr {
    nullable_text_key(value).eq(text_literal(""))
}

#[cfg(test)]
mod tests {
    use crate::nullable_text_key::{is_null_text_key, nullable_text_key, same_nullable_text_key};
    use sea_query::{Alias, Expr, PostgresQueryBuilder, Query, SimpleExpr};

    fn render(expression: SimpleExpr) -> String {
        Query::select().expr(expression).to_owned().to_string(PostgresQueryBuilder)
    }

    #[test]
    fn a_key_member_reads_null_as_the_inline_empty_string() {
        let sql = render(nullable_text_key(Expr::col(Alias::new("dataset_id"))));
        assert_eq!(sql, r#"SELECT COALESCE("dataset_id", '')"#);
    }

    #[test]
    fn two_key_members_compare_with_plain_equality() {
        let sql = render(same_nullable_text_key(
            Expr::col((Alias::new("a"), Alias::new("dataset_id"))),
            Expr::col((Alias::new("b"), Alias::new("dataset_id"))),
        ));
        assert_eq!(sql, r#"SELECT COALESCE("a"."dataset_id", '') = COALESCE("b"."dataset_id", '')"#);
        assert!(!sql.contains("DISTINCT"), "{sql}");
    }

    #[test]
    fn a_null_member_is_tested_through_the_key() {
        let sql = render(is_null_text_key(Expr::col(Alias::new("dataset_id"))));
        assert_eq!(sql, r#"SELECT COALESCE("dataset_id", '') = ('')"#);
    }
}
