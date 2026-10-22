//! `EXPLAIN` over a statement, to read the plan PostgreSQL chooses for it.
//!
//! The explained statement keeps its own bound values, so the plan is the one
//! the planner builds for those values (or, with [`ExplainOption::GenericPlan`],
//! the one it builds for any). The answer is one text row per plan line.

use crate::statement::{Statement, rendering::Rendered};
use sea_query::Values;
use strum::Display;

/// One option of an `EXPLAIN` command.
#[derive(Clone, Copy, Debug, Display, Eq, PartialEq)]
pub enum ExplainOption {
    /// `ANALYZE`: run the statement and report its actual rows and timings.
    #[strum(serialize = "ANALYZE")]
    Analyze,
    /// `BUFFERS`: report the buffers the statement touched.
    #[strum(serialize = "BUFFERS")]
    Buffers,
    /// `GENERIC_PLAN`: plan the statement for any parameter values.
    #[strum(serialize = "GENERIC_PLAN")]
    GenericPlan,
}

/// `EXPLAIN [(options)] <statement>`.
pub struct Explain<S> {
    /// The statement explained.
    statement: S,
    /// The options, in the order given.
    options: Vec<ExplainOption>,
}

impl<S> Explain<S> {
    /// The plan of `statement`, without options.
    pub const fn new(statement: S) -> Explain<S> {
        Explain {
            statement,
            options: Vec::new(),
        }
    }

    /// The same explanation with `option` added.
    #[must_use]
    pub fn with(mut self, option: ExplainOption) -> Explain<S> {
        self.options.push(option);
        self
    }
}

impl<S: Rendered> Rendered for Explain<S> {
    fn rendered(&self) -> (String, Values) {
        let (sql, values) = self.statement.rendered();
        let options = if self.options.is_empty() {
            String::new()
        } else {
            format!("({}) ", self.options.iter().map(ToString::to_string).collect::<Vec<_>>().join(", "))
        };
        (format!("EXPLAIN {options}{sql}"), values)
    }
}

impl<S: Statement> Statement for Explain<S> {}

#[cfg(test)]
mod tests {
    use crate::{
        explain::{Explain, ExplainOption},
        statement::rendering::Rendered,
    };
    use pretty_assertions::assert_eq;
    use sea_query::{Alias, Expr, ExprTrait, Query};

    fn lookup() -> sea_query::SelectStatement {
        Query::select()
            .column(Alias::new("id"))
            .from(Alias::new("entities"))
            .and_where(Expr::col(Alias::new("id")).eq("urn:a"))
            .to_owned()
    }

    #[test]
    fn a_plain_explanation_prefixes_the_statement_and_keeps_its_values() {
        let (sql, values) = Explain::new(lookup()).rendered();
        assert_eq!(sql, r#"EXPLAIN SELECT "id" FROM "entities" WHERE "id" = $1"#);
        assert_eq!(values.0.len(), 1);
    }

    #[test]
    fn the_options_are_listed_in_the_order_given() {
        let (sql, _) = Explain::new(lookup()).with(ExplainOption::Analyze).with(ExplainOption::Buffers).rendered();
        assert_eq!(sql, r#"EXPLAIN (ANALYZE, BUFFERS) SELECT "id" FROM "entities" WHERE "id" = $1"#);
        let (sql, _) = Explain::new(lookup()).with(ExplainOption::GenericPlan).rendered();
        assert_eq!(sql, r#"EXPLAIN (GENERIC_PLAN) SELECT "id" FROM "entities" WHERE "id" = $1"#);
    }
}
