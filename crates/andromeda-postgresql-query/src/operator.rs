//! How clause 4.9's comparison and pattern operators are spelled in PostgreSQL.

use andromeda_ngsi_ld_query::{
    filter::predicate::{CmpOp, PatternPolarity},
    pattern::Pattern,
    subject_start::SubjectStart,
};
use andromeda_postgresql_seaquery::{
    collation::{PostgresCollation, collate},
    operator::negate_total,
    pg_operator::PgBinaryOperator,
};
use sea_query::{BinOper, Condition, Expr, ExprTrait, SimpleExpr};

/// Build a clause 4.9 comparison between two already-lifted operands.
///
/// Shared by every rendering of clause 4.9, so a predicate cannot compare with
/// one operator against `current_attributes`, another against a JSON value
/// and a third against a registration's properties. The operator token differs
/// from the clause 4.9 spelling only at `==`, which SQL writes `=`, and `!=`,
/// which is kept over the equivalent `<>`.
#[must_use]
pub fn compare(lhs: SimpleExpr, op: CmpOp, rhs: SimpleExpr) -> SimpleExpr {
    let lhs = Expr::expr(lhs);
    match op {
        CmpOp::Equal => lhs.eq(rhs),
        CmpOp::Unequal => lhs.binary(BinOper::Custom("!="), rhs),
        CmpOp::Greater => lhs.gt(rhs),
        CmpOp::GreaterEq => lhs.gte(rhs),
        CmpOp::Less => lhs.lt(rhs),
        CmpOp::LessEq => lhs.lte(rhs),
    }
}

/// Whether a comparison does not hold, counting an unknown as a non-match.
///
/// SQL's `NOT` propagates unknown: a comparison against a column the row leaves
/// empty is NULL, and `NOT NULL` is NULL, which the enclosing `WHERE` drops.
/// Clause 4.9's `unequal` requires the opposite reading: "If the data type of
/// the target value and the data type of the Query Term value are different,
/// then they shall be considered unequal", so a target the comparison could not
/// even be evaluated against is a match, not a silent rejection.
#[must_use]
pub fn does_not_hold(comparison: Condition) -> SimpleExpr {
    negate_total(SimpleExpr::from(comparison))
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
/// answers it reads.
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
pub fn pattern(target: SimpleExpr, polarity: PatternPolarity, pattern: &Pattern, indexes: PatternIndexes) -> SimpleExpr {
    let target = match (indexes, pattern.subject_start()) {
        (PatternIndexes::None, SubjectStart::Fixed | SubjectStart::Open) | (PatternIndexes::TrigramsAndByteOrder, SubjectStart::Fixed) => target,
        (PatternIndexes::Trigrams, SubjectStart::Fixed | SubjectStart::Open) | (PatternIndexes::TrigramsAndByteOrder, SubjectStart::Open) => {
            collate(target, &PostgresCollation::database_default())
        }
    };
    let restatement = Expr::val(pattern.portable().as_str());
    match polarity {
        PatternPolarity::Match => target.posix_matches(restatement),
        PatternPolarity::NotMatch => target.posix_not_matches(restatement),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use andromeda_ngsi_ld_query::filter::predicate::{OrderingOp, SetMembership};
    use pretty_assertions::assert_eq;
    use sea_query::{PostgresQueryBuilder, Query};

    fn render(expr: SimpleExpr) -> String {
        Query::select().expr(expr).to_owned().to_string(PostgresQueryBuilder)
    }

    fn operands() -> (SimpleExpr, SimpleExpr) {
        (Expr::col(sea_query::Alias::new("stored")), Expr::val("x"))
    }

    #[test]
    fn equality_is_the_one_operator_sql_spells_differently_from_clause_4_9() {
        let (lhs, rhs) = operands();
        assert!(render(compare(lhs, SetMembership::Within.as_cmp_op(), rhs)).contains(" = "));
        assert_eq!(SetMembership::Within.ngsi_token(), "==");
    }

    #[test]
    fn inequality_keeps_the_bang_equals_spelling() {
        let (lhs, rhs) = operands();
        assert!(render(compare(lhs, SetMembership::Outside.as_cmp_op(), rhs)).contains(" != "));
    }

    #[test]
    fn each_ordering_operator_renders_its_own_token() {
        for (op, token) in [
            (OrderingOp::Greater, " > "),
            (OrderingOp::GreaterEq, " >= "),
            (OrderingOp::Less, " < "),
            (OrderingOp::LessEq, " <= "),
        ] {
            let (lhs, rhs) = operands();
            assert!(render(compare(lhs, op.as_cmp_op(), rhs)).contains(token), "{op:?}");
        }
    }

    fn pattern_on_stored(raw: &str, polarity: PatternPolarity, indexes: PatternIndexes) -> String {
        let parsed = Pattern::parse(raw).expect("valid pattern");
        render(pattern(Expr::col(sea_query::Alias::new("stored")), polarity, &parsed, indexes))
    }

    #[test]
    fn a_pattern_predicate_renders_the_posix_operator_of_its_polarity() {
        assert!(pattern_on_stored("abc", PatternPolarity::Match, PatternIndexes::None).contains(r#""stored" ~ 'abc'"#));
        assert!(pattern_on_stored("abc", PatternPolarity::NotMatch, PatternIndexes::None).contains(r#""stored" !~ 'abc'"#));
    }

    /// The store runs the restatement, never the client's text.
    #[test]
    fn a_pattern_predicate_tests_the_portable_restatement() {
        assert!(pattern_on_stored("(?i)čer", PatternPolarity::Match, PatternIndexes::None).ends_with(" ~ '[Čč][Ee][Rr]'"));
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
        for (raw, indexes) in answered {
            for polarity in [PatternPolarity::Match, PatternPolarity::NotMatch] {
                let sql = pattern_on_stored(raw, polarity, indexes);
                assert!(sql.contains(r#"(("stored") COLLATE "default") "#), "{raw} {indexes:?}: {sql}");
            }
        }
    }

    /// PostgreSQL reads a fixed start off a B-tree only under the column's own
    /// `"C"` collation.
    #[test]
    fn a_pattern_fixing_the_start_of_byte_ordered_text_keeps_the_column_collation() {
        for raw in FIXED {
            let sql = pattern_on_stored(raw, PatternPolarity::Match, PatternIndexes::TrigramsAndByteOrder);
            assert!(sql.contains(r#""stored" ~ '^"#) && !sql.contains("COLLATE"), "{raw}: {sql}");
        }
    }

    #[test]
    fn a_pattern_no_index_answers_keeps_the_target_collation() {
        for raw in OPEN.iter().chain(&FIXED) {
            assert!(
                !pattern_on_stored(raw, PatternPolarity::Match, PatternIndexes::None).contains("COLLATE"),
                "{raw}"
            );
        }
    }
}
