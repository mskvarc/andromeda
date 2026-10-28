//! The PL/pgSQL syntax tree of a function body or a `do` block, as PostgreSQL's
//! own PL/pgSQL parser reads it.
//!
//! The parser reports its tree as JSON, which is read straight into the typed
//! nodes below. Every statement kind PL/pgSQL has is a variant, and each node
//! keeps only what a rule reads: the statements it nests, the SQL expressions
//! it embeds, and the conditions an exception handler catches. Fields a rule
//! never reads are skipped as the JSON is read.
//!
//! The JSON writer of the parser emits an empty object for the implicit
//! variables a trigger function declares (`tg_table_name` and its kind), which
//! is not valid JSON. A trigger function is therefore read through a stand-in
//! that declares the same body returning `integer`: the statements and
//! expressions are the same, only the implicit variables are absent.

/// The block structure of a routine.
mod block;
/// Every statement kind.
mod statement;

use crate::plpgsql_tree::block::{ExceptionNode, Expression, ExpressionNode, FunctionNode};
use pg_query::ParserOptions;
use serde::Deserialize;
use std::ops::Range;

/// One routine the PL/pgSQL parser read: a function body or a `do` block.
#[derive(Debug, Deserialize)]
pub struct PlpgsqlRoutine {
    /// The routine's node.
    #[serde(rename = "PLpgSQL_function")]
    function: FunctionNode,
}

impl PlpgsqlRoutine {
    /// The routines of `statement`, a `create function` or a `do` statement.
    ///
    /// `trigger_return` is the byte range of the `trigger` return type within
    /// `statement`, for a trigger function, which is read through its stand-in.
    ///
    /// # Errors
    ///
    /// Returns [`PlpgsqlError`] when the parser refuses the body or reports a
    /// tree that is not the documented one.
    pub fn parse(statement: &str, trigger_return: Option<Range<usize>>) -> Result<Vec<PlpgsqlRoutine>, PlpgsqlError> {
        let stand_in;
        let statement = match trigger_return {
            Some(range) => {
                stand_in = format!("{}integer{}", &statement[..range.start], &statement[range.end..]);
                stand_in.as_str()
            }
            None => statement,
        };
        let tree = pg_query::parse_plpgsql(statement).map_err(PlpgsqlError::Unparsed)?;
        Vec::<PlpgsqlRoutine>::deserialize(tree).map_err(PlpgsqlError::Unreadable)
    }

    /// Every SQL expression the routine embeds, in the order written.
    #[must_use]
    pub fn embedded_sql(&self) -> Vec<EmbeddedSql<'_>> {
        let mut found = Vec::new();
        self.function.action.block.collect(&mut Collected::Sql(&mut found));
        found
    }

    /// The name of every condition an exception handler of the routine catches.
    #[must_use]
    pub fn caught_conditions(&self) -> Vec<&str> {
        let mut found = Vec::new();
        self.function.action.block.collect(&mut Collected::Conditions(&mut found));
        found
    }
}

/// What a walk of the tree gathers.
enum Collected<'tree, 'out> {
    /// The embedded SQL expressions.
    Sql(&'out mut Vec<EmbeddedSql<'tree>>),
    /// The names of the conditions exception handlers catch.
    Conditions(&'out mut Vec<&'tree str>),
}

impl<'tree> Collected<'tree, '_> {
    /// Record `expression`, read as `role`, when SQL is gathered.
    fn expression(&mut self, expression: Option<&'tree ExpressionNode>, role: SqlRole) {
        if let (Collected::Sql(found), Some(expression)) = (self, expression) {
            found.push(EmbeddedSql {
                expression: &expression.expression,
                role,
            });
        }
    }

    /// Record every expression of `expressions` as plain values.
    fn values(&mut self, expressions: &'tree [ExpressionNode]) {
        for expression in expressions {
            self.expression(Some(expression), SqlRole::Static);
        }
    }

    /// Record every condition of `handler`, when conditions are gathered.
    fn handler(&mut self, handler: &'tree ExceptionNode) {
        if let Collected::Conditions(found) = self {
            found.extend(handler.exception.conditions.iter().map(|condition| condition.condition.condname.as_str()));
        }
    }
}

/// How an embedded expression reaches the server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SqlRole {
    /// Planned as written: a statement, a condition or a value.
    Static,
    /// Evaluated to the text of a command that is then executed.
    DynamicCommand,
}

/// One SQL expression a routine embeds.
#[derive(Debug, Clone, Copy)]
pub struct EmbeddedSql<'tree> {
    /// The expression.
    expression: &'tree Expression,
    /// How it reaches the server.
    role: SqlRole,
}

impl<'tree> EmbeddedSql<'tree> {
    /// The expression as written.
    #[must_use]
    pub fn query(&self) -> &'tree str {
        &self.expression.query
    }

    /// The grammar the expression is written in.
    #[must_use]
    pub fn parse_options(&self) -> ParserOptions {
        ParserOptions::from(self.expression.parse_mode)
    }

    /// How it reaches the server.
    #[must_use]
    pub const fn role(&self) -> SqlRole {
        self.role
    }
}

/// Why a PL/pgSQL body could not be read.
#[derive(Debug, thiserror::Error)]
pub enum PlpgsqlError {
    /// The PL/pgSQL parser refused the body.
    #[error("the PL/pgSQL body does not parse: {0}")]
    Unparsed(pg_query::Error),
    /// The parser reported a tree that is not the documented one.
    #[error("the PL/pgSQL tree could not be read: {0}")]
    Unreadable(serde_json::Error),
}

#[cfg(test)]
mod tests {
    use crate::plpgsql_tree::{PlpgsqlError, PlpgsqlRoutine, SqlRole};
    use pretty_assertions::assert_eq;

    const TRIGGER_FUNCTION: &str = "create function f ()\n    returns trigger\n    language plpgsql\n    as $$\nbegin\n    execute format('delete from %I.t', tg_table_schema);\n    return null;\nend\n$$";

    const CATCH_ALL: &str = "do $$\nbegin\n    create extension if not exists x;\nexception\n    when others then\n        raise notice 'skipped';\nend\n$$";

    #[test]
    fn a_trigger_function_is_read_through_its_stand_in() -> Result<(), PlpgsqlError> {
        let start = TRIGGER_FUNCTION.find("trigger").unwrap_or_default();
        let routines = PlpgsqlRoutine::parse(TRIGGER_FUNCTION, Some(start..start + "trigger".len()))?;
        let sql: Vec<(&str, SqlRole)> = routines
            .iter()
            .flat_map(PlpgsqlRoutine::embedded_sql)
            .map(|sql| (sql.query(), sql.role()))
            .collect();
        assert_eq!(
            sql,
            [
                ("format('delete from %I.t', tg_table_schema)", SqlRole::DynamicCommand),
                ("null", SqlRole::Static)
            ]
        );
        Ok(())
    }

    #[test]
    fn a_catch_all_handler_names_others() -> Result<(), PlpgsqlError> {
        let routines = PlpgsqlRoutine::parse(CATCH_ALL, None)?;
        let conditions: Vec<&str> = routines.iter().flat_map(PlpgsqlRoutine::caught_conditions).collect();
        assert_eq!(conditions, ["others"]);
        Ok(())
    }
}
