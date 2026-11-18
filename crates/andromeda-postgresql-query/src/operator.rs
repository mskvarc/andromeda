//! How clause 4.9's comparison and pattern operators are spelled in PostgreSQL.

use andromeda_ngsi_ld_query::{
    filter::reading::{comparator::Comparator, order_relation::OrderRelation},
    pattern::{Pattern, subject_start::SubjectStart},
};
use andromeda_postgresql_seaquery::{
    collation::{PostgresCollation, collate},
    pg_operator::PgBinaryOperator,
};
use sea_query::{Expr, ExprTrait, SimpleExpr};

/// Build one clause 4.9 comparison between two already-lifted operands.
///
/// Shared by every rendering of clause 4.9, so a term cannot compare with one
/// operator against `current_attributes`, another against a JSON value and a
/// third against a registration's properties. The operator token differs from
/// the clause 4.9 spelling only at `==`, which SQL writes `=`. No comparison is
/// unequal: `!=` is the complement of `==`, which
/// [`verdict_condition`](crate::q_predicate::verdict_condition) states.
#[must_use]
pub fn compare(lhs: SimpleExpr, comparator: Comparator, rhs: SimpleExpr) -> SimpleExpr {
    let lhs = Expr::expr(lhs);
    match comparator {
        Comparator::Equal => lhs.eq(rhs),
        Comparator::Ordered(OrderRelation::Greater) => lhs.gt(rhs),
        Comparator::Ordered(OrderRelation::GreaterOrEqual) => lhs.gte(rhs),
        Comparator::Ordered(OrderRelation::Less) => lhs.lt(rhs),
        Comparator::Ordered(OrderRelation::LessOrEqual) => lhs.lte(rhs),
    }
}

/// The indexes over the stored text a pattern is tested against, which
/// decide the collation [`pattern`] tests it under.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PatternIndexes {
    /// No index answers a pattern over the text.
    None,
    /// A trigram index alone.
    Trigrams,
    /// A trigram index, and a B-tree over the text in its own byte order.
    TrigramsAndByteOrder,
}

/// Test `target` against the portable restatement of `pattern`, a clause 4.9
/// `RegExp` or a clause 5.7.2 `idPattern`, under the collation the index that
/// answers it reads. `!~=` is the complement of the asserted match, which
/// [`verdict_condition`](crate::q_predicate::verdict_condition) states.
///
/// The restatement means the same under every collation, so the collation
/// decides only which index can answer the test:
///
/// * A B-tree answers a pattern that fixes the start of its subject as a range
///   of the byte order every stored string column declares, which PostgreSQL
///   derives only when the test runs under that `"C"` collation.
/// * A trigram index yields the rows holding the trigrams `pg_trgm` finds in
///   the pattern. `pg_trgm` classifies the characters of an indexed string by
///   the database's default collation, and those of the pattern by the
///   collation the test runs under. Under `"C"` in a database whose default
///   classifies `Č` as a letter the two disagree, and the index drops strings
///   the pattern matches: `[OČ]abc` misses `Čabc`, `LJE ŽP` misses `x LJE ŽP`.
///   Every trigram index column declares the database's default, and a test
///   it answers runs under it, so both sides classify alike.
#[must_use]
pub fn pattern(target: SimpleExpr, pattern: &Pattern, indexes: PatternIndexes) -> SimpleExpr {
    let target = match (indexes, pattern.subject_start()) {
        (PatternIndexes::None, SubjectStart::Fixed | SubjectStart::Open) | (PatternIndexes::TrigramsAndByteOrder, SubjectStart::Fixed) => target,
        (PatternIndexes::Trigrams, SubjectStart::Fixed | SubjectStart::Open) | (PatternIndexes::TrigramsAndByteOrder, SubjectStart::Open) => {
            collate(target, &PostgresCollation::database_default())
        }
    };
    target.posix_matches(Expr::val(pattern.portable().as_str()))
}

#[cfg(test)]
mod tests {
    use crate::operator::{PatternIndexes, compare, pattern};
    use andromeda_ngsi_ld_query::{
        filter::reading::{comparator::Comparator, order_relation::OrderRelation},
        pattern::Pattern,
    };
    use andromeda_postgresql_seaquery::assert_statement_snapshot;
    use sea_query::{Expr, Query, SelectStatement, SimpleExpr};

    fn render(expr: SimpleExpr) -> SelectStatement {
        Query::select().expr(expr).to_owned()
    }

    fn operands() -> (SimpleExpr, SimpleExpr) {
        (Expr::col(sea_query::Alias::new("stored")), Expr::val("x"))
    }

    #[test]
    fn equality_is_the_one_operator_sql_spells_differently_from_clause_4_9() {
        let (lhs, rhs) = operands();
        assert_statement_snapshot!(
            "operator-equality-is-the-one-operator-sql-spells-differently-from-clause-4-9",
            render(compare(lhs, Comparator::Equal, rhs))
        );
    }

    #[test]
    fn each_ordering_operator_renders_its_own_token() {
        for (index, relation) in [
            OrderRelation::Greater,
            OrderRelation::GreaterOrEqual,
            OrderRelation::Less,
            OrderRelation::LessOrEqual,
        ]
        .into_iter()
        .enumerate()
        {
            let (lhs, rhs) = operands();
            assert_statement_snapshot!(
                format!("operator-each-ordering-operator-renders-its-own-token-{}", index + 1),
                render(compare(lhs, Comparator::Ordered(relation), rhs))
            );
        }
    }

    fn pattern_on_stored(raw: &str, indexes: PatternIndexes) -> SelectStatement {
        let parsed = Pattern::parse(raw).expect("valid pattern");
        render(pattern(Expr::col(sea_query::Alias::new("stored")), &parsed, indexes))
    }

    #[test]
    fn a_pattern_predicate_renders_the_posix_match_operator() {
        assert_statement_snapshot!(
            "operator-a-pattern-predicate-renders-the-posix-match-operator",
            pattern_on_stored("abc", PatternIndexes::None)
        );
    }

    /// The store runs the restatement, never the client's text.
    #[test]
    fn a_pattern_predicate_tests_the_portable_restatement() {
        assert_statement_snapshot!(
            "operator-a-pattern-predicate-tests-the-portable-restatement",
            pattern_on_stored("(?i)čer", PatternIndexes::None)
        );
    }

    const OPEN: [&str; 5] = ["^(O|Č)", "Nám\\. Ľ.*a", "LJE ŽP$", "[OČ]abc", "(?i)^č"];

    const FIXED: [&str; 3] = ["^Čab", "^urn:ngsi-ld:Stop:.*", "^Šte$"];

    /// Every trigram index column declares the database's default collation.
    #[test]
    fn a_pattern_a_trigram_index_answers_is_tested_under_the_database_default() {
        let answered = OPEN
            .iter()
            .map(|raw| (*raw, PatternIndexes::TrigramsAndByteOrder))
            .chain(OPEN.iter().chain(&FIXED).map(|raw| (*raw, PatternIndexes::Trigrams)));
        for (index, (raw, indexes)) in answered.enumerate() {
            assert_statement_snapshot!(
                format!("operator-a-pattern-a-trigram-index-answers-is-tested-under-the-database-default-{}", index + 1),
                pattern_on_stored(raw, indexes)
            );
        }
    }

    /// PostgreSQL reads a fixed start off a B-tree only under the column's own
    /// `"C"` collation.
    #[test]
    fn a_pattern_fixing_the_start_of_byte_ordered_text_keeps_the_column_collation() {
        for (index, raw) in FIXED.into_iter().enumerate() {
            assert_statement_snapshot!(
                format!(
                    "operator-a-pattern-fixing-the-start-of-byte-ordered-text-keeps-the-column-collation-{}",
                    index + 1
                ),
                pattern_on_stored(raw, PatternIndexes::TrigramsAndByteOrder)
            );
        }
    }

    #[test]
    fn a_pattern_no_index_answers_keeps_the_target_collation() {
        for (index, raw) in OPEN.iter().chain(&FIXED).enumerate() {
            assert_statement_snapshot!(
                format!("operator-a-pattern-no-index-answers-keeps-the-target-collation-{}", index + 1),
                pattern_on_stored(raw, PatternIndexes::None)
            );
        }
    }
}
