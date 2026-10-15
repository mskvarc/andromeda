//! The two readings of an instance's own value that an ordering or a range
//! compares (clause 4.9): the typed column holding the operand's kind and, for
//! an operand a string can compare with, every string of a `languageMap`.
//!
//! The term's condition is their disjunction. The index path of the term reads
//! each arm on its own, so both are built here once and neither can drift from
//! the condition the page rechecks.

use sea_query::{Condition, SimpleExpr};

/// The readings of an instance's own value a comparison holds over.
#[derive(Debug, Clone)]
pub struct OwnValueArms {
    /// The comparison against the typed column holding the operand's kind.
    pub typed: Condition,
    /// The comparison against the strings of a `languageMap`, `None` for an
    /// operand no string compares with (a number or a boolean).
    pub language: Option<SimpleExpr>,
}

impl OwnValueArms {
    /// The term's condition: some reading holds.
    #[must_use]
    pub fn either(self) -> Condition {
        match self.language {
            Some(language) => Condition::any().add(self.typed).add(language),
            None => self.typed,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::q_predicate::own_value_arms::OwnValueArms;
    use sea_query::{Alias, Condition, Expr, ExprTrait, PostgresQueryBuilder, Query};

    fn render(arms: OwnValueArms) -> String {
        Query::select().cond_where(arms.either()).to_owned().to_string(PostgresQueryBuilder)
    }

    #[test]
    fn both_readings_are_disjoined() {
        let arms = OwnValueArms {
            typed: Condition::all().add(Expr::col(Alias::new("t")).gt(1)),
            language: Some(Expr::col(Alias::new("l")).gt(2)),
        };
        assert!(render(arms).ends_with(r#"WHERE "t" > 1 OR "l" > 2"#));
    }

    #[test]
    fn an_operand_no_string_compares_with_reads_the_typed_column_alone() {
        let arms = OwnValueArms {
            typed: Condition::all().add(Expr::col(Alias::new("t")).gt(1)),
            language: None,
        };
        assert!(render(arms).ends_with(r#"WHERE "t" > 1"#));
    }
}
