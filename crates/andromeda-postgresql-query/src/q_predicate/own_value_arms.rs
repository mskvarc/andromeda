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
    /// The comparison against each typed field's columns alone, one field
    /// each, without the value kind a field is read beside, which every row
    /// `typed` keeps passes
    /// ([`typed_field_keys`](crate::q_predicate::typed_column::typed_field_keys)).
    pub keys: Vec<SimpleExpr>,
    /// The comparison against the strings of a `languageMap`, `None` for an
    /// operand no language string compares with (a number, a boolean or a time
    /// value).
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
    use andromeda_postgresql_seaquery::assert_statement_snapshot;
    use sea_query::{Alias, Condition, Expr, ExprTrait, Query, SelectStatement};

    fn render(arms: OwnValueArms) -> SelectStatement {
        Query::select().cond_where(arms.either()).to_owned()
    }

    #[test]
    fn both_readings_are_disjoined() {
        let arms = OwnValueArms {
            typed: Condition::all().add(Expr::col(Alias::new("t")).gt(1)),
            keys: vec![Expr::col(Alias::new("t")).gt(1)],
            language: Some(Expr::col(Alias::new("l")).gt(2)),
        };
        assert_statement_snapshot!("own-value-arms-both-readings-are-disjoined", render(arms));
    }

    #[test]
    fn an_operand_no_string_compares_with_reads_the_typed_column_alone() {
        let arms = OwnValueArms {
            typed: Condition::all().add(Expr::col(Alias::new("t")).gt(1)),
            keys: vec![Expr::col(Alias::new("t")).gt(1)],
            language: None,
        };
        assert_statement_snapshot!("own-value-arms-an-operand-no-string-compares-with-reads-the-typed-column-alone", render(arms));
    }
}
