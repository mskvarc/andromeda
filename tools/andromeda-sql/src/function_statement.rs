//! The `CREATE FUNCTION` statements of a migration, held out of `pg_format` and
//! put back as written.
//!
//! `pg_format` reformats a `plpgsql` body as plain SQL, unwraps a body written
//! as a string literal, and a `language plpgsql` clause can shift the
//! indentation of earlier comments. Each function statement (a `CREATE
//! PROCEDURE` parses to the same statement kind and is held out with them) is
//! therefore swapped for a placeholder statement selecting a string literal
//! that numbers it, which `pg_format` lays out like any other statement, and is
//! put back verbatim once the text is formatted.

use crate::sql_statement::{SqlStatement, SqlStatementError};
use pg_query::{NodeEnum, protobuf::a_const::Val};
use std::ops::Range;

/// What a placeholder's string literal starts with.
const PLACEHOLDER_PREFIX: &str = "__function_statement_";

/// What a placeholder's string literal ends with.
const PLACEHOLDER_SUFFIX: &str = "__";

/// A migration's text with every function statement swapped for a placeholder.
#[derive(Debug)]
pub struct HeldOutFunctions<'sql> {
    /// The text with placeholders in place of the function statements.
    shielded: String,
    /// The function statements, verbatim, in the order they were written.
    statements: Vec<&'sql str>,
}

impl<'sql> HeldOutFunctions<'sql> {
    /// Replace every function statement of `sql` with a placeholder.
    ///
    /// # Errors
    ///
    /// Returns [`FunctionStatementError`] when `sql` does not parse.
    pub fn hold_out(sql: &'sql str) -> Result<HeldOutFunctions<'sql>, FunctionStatementError> {
        let mut shielded = String::with_capacity(sql.len());
        let mut statements = Vec::new();
        let mut copied = 0;
        for (span, kind) in parsed_statements(sql).map_err(FunctionStatementError::Unparsed)? {
            if matches!(kind, StatementKind::Function) {
                shielded.push_str(&sql[copied..span.start]);
                shielded.push_str(&placeholder(statements.len()));
                statements.push(&sql[span.start..span.end]);
                copied = span.end;
            }
        }
        shielded.push_str(&sql[copied..]);
        Ok(HeldOutFunctions { shielded, statements })
    }

    /// The text to format, with placeholders in place of the function statements.
    #[must_use]
    pub fn shielded(&self) -> &str {
        &self.shielded
    }

    /// Put the held-out statements back in place of the placeholders of
    /// `formatted`.
    ///
    /// # Errors
    ///
    /// Returns [`FunctionStatementError`] when `formatted` does not parse, or
    /// its placeholders are not exactly the ones held out, in order.
    pub fn restore(&self, formatted: &str) -> Result<String, FunctionStatementError> {
        let mut restored = String::with_capacity(formatted.len() + self.statements.iter().map(|statement| statement.len()).sum::<usize>());
        let mut found = Vec::new();
        let mut copied = 0;
        for (span, kind) in parsed_statements(formatted).map_err(FunctionStatementError::FormattedUnparsed)? {
            if let StatementKind::Placeholder(position) = kind {
                if let Some(original) = self.statements.get(position).filter(|_| position == found.len()) {
                    restored.push_str(&formatted[copied..span.start]);
                    restored.push_str(original);
                    copied = span.end;
                }
                found.push(position);
            }
        }
        if let Some((expected, returned)) = found.iter().enumerate().find(|(index, position)| index != *position) {
            return Err(FunctionStatementError::Reordered { expected, returned: *returned });
        }
        if found.len() != self.statements.len() {
            return Err(FunctionStatementError::Miscounted {
                held: self.statements.len(),
                returned: found.len(),
            });
        }
        restored.push_str(&formatted[copied..]);
        Ok(restored)
    }
}

/// The placeholder statement standing for the held-out statement at `position`.
fn placeholder(position: usize) -> String {
    format!("select '{PLACEHOLDER_PREFIX}{position}{PLACEHOLDER_SUFFIX}';")
}

/// What one top-level statement is, as far as the hold-out is concerned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StatementKind {
    /// A `CREATE FUNCTION` or `CREATE PROCEDURE` statement.
    Function,
    /// A placeholder, numbering the held-out statement it stands for.
    Placeholder(usize),
    /// Any other statement.
    Other,
}

/// The span and kind of every top-level statement of `sql`, in order.
fn parsed_statements(sql: &str) -> Result<Vec<(Range<usize>, StatementKind)>, SqlStatementError> {
    let tokens = pg_query::scan(sql).map_err(SqlStatementError::Unparsed)?.tokens;
    Ok(SqlStatement::parse_all(sql, &tokens)?
        .iter()
        .map(|statement| (statement.span(), statement_kind(statement.node())))
        .collect())
}

/// Whether the statement is a function statement, a placeholder, or neither.
fn statement_kind(statement: Option<&NodeEnum>) -> StatementKind {
    match statement {
        Some(NodeEnum::CreateFunctionStmt(_)) => StatementKind::Function,
        Some(NodeEnum::SelectStmt(select)) if select.from_clause.is_empty() => match select.target_list.as_slice() {
            [target] => placeholder_position(target).map_or(StatementKind::Other, StatementKind::Placeholder),
            _ => StatementKind::Other,
        },
        Some(_) | None => StatementKind::Other,
    }
}

/// The position a placeholder's selected string literal numbers.
fn placeholder_position(target: &pg_query::Node) -> Option<usize> {
    let Some(NodeEnum::ResTarget(target)) = &target.node else {
        return None;
    };
    let Some(NodeEnum::AConst(constant)) = target.val.as_ref().and_then(|value| value.node.as_ref()) else {
        return None;
    };
    let Some(Val::Sval(literal)) = &constant.val else {
        return None;
    };
    literal.sval.strip_prefix(PLACEHOLDER_PREFIX)?.strip_suffix(PLACEHOLDER_SUFFIX)?.parse().ok()
}

/// Why the function statements could not be held out or put back.
#[derive(Debug, thiserror::Error)]
pub enum FunctionStatementError {
    /// The migration's statements could not be read.
    #[error("the migration's statements could not be read: {0}")]
    Unparsed(SqlStatementError),
    /// The statements of the text `pg_format` returned could not be read.
    #[error("the statements of the text pg_format returned could not be read: {0}")]
    FormattedUnparsed(SqlStatementError),
    /// `pg_format` returned a placeholder out of the order it was held out in.
    #[error("pg_format returned function statement placeholder {returned} where placeholder {expected} was held out")]
    Reordered {
        /// The position the placeholder at this place was held out with.
        expected: usize,
        /// The position the placeholder returned there numbers.
        returned: usize,
    },
    /// `pg_format` returned fewer or more placeholders than were held out.
    #[error("pg_format returned {returned} function statement placeholders where {held} were held out")]
    Miscounted {
        /// How many statements were held out.
        held: usize,
        /// How many placeholders came back.
        returned: usize,
    },
}

#[cfg(test)]
mod tests {
    use crate::function_statement::{FunctionStatementError, HeldOutFunctions};
    use pretty_assertions::assert_eq;

    const PLPGSQL_BLOCK: &str =
        "create function refuse_change ()\n    returns trigger\n    language plpgsql\n    as $$\nbegin\n    raise   exception 'no';\nend;\n$$;";

    const STRING_LITERAL_BODY: &str = "create or replace function double_it (x integer)\n    returns integer\n    language sql\n    as 'select  x *  2';";

    fn migration() -> String {
        format!("-- Refuses a change.\n{PLPGSQL_BLOCK}\n\ncreate table t (a integer);\n\n-- Doubles.\n{STRING_LITERAL_BODY}\n")
    }

    #[test]
    fn every_function_statement_is_replaced_by_a_numbered_placeholder() -> Result<(), FunctionStatementError> {
        let migration = migration();
        let held = HeldOutFunctions::hold_out(&migration)?;
        assert_eq!(
            held.shielded(),
            "-- Refuses a change.\nselect '__function_statement_0__';\n\ncreate table t (a integer);\n\n-- Doubles.\nselect '__function_statement_1__';\n"
        );
        Ok(())
    }

    #[test]
    fn a_plpgsql_block_and_a_string_literal_body_come_back_unchanged() -> Result<(), FunctionStatementError> {
        let migration = migration();
        let held = HeldOutFunctions::hold_out(&migration)?;
        let formatted = "-- Refuses a change.\nSELECT\n    '__function_statement_0__';\n\nCREATE TABLE t (\n    a integer\n);\n\n-- Doubles.\nSELECT\n    '__function_statement_1__';\n\n";
        assert_eq!(
            held.restore(formatted)?,
            format!("-- Refuses a change.\n{PLPGSQL_BLOCK}\n\nCREATE TABLE t (\n    a integer\n);\n\n-- Doubles.\n{STRING_LITERAL_BODY}\n\n")
        );
        Ok(())
    }

    #[test]
    fn a_lost_placeholder_is_refused() -> Result<(), FunctionStatementError> {
        let migration = migration();
        let held = HeldOutFunctions::hold_out(&migration)?;
        let restored = held.restore("select '__function_statement_0__';\n");
        assert!(
            matches!(&restored, Err(FunctionStatementError::Miscounted { held: 2, returned: 1 })),
            "{restored:?}"
        );
        Ok(())
    }

    #[test]
    fn reordered_placeholders_are_refused() -> Result<(), FunctionStatementError> {
        let migration = migration();
        let held = HeldOutFunctions::hold_out(&migration)?;
        let restored = held.restore("select '__function_statement_1__';\nselect '__function_statement_0__';\n");
        assert!(
            matches!(&restored, Err(FunctionStatementError::Reordered { expected: 0, returned: 1 })),
            "{restored:?}"
        );
        Ok(())
    }

    #[test]
    fn a_text_without_function_statements_is_returned_as_is() -> Result<(), FunctionStatementError> {
        let migration = "create table t (a integer);\n-- create function in a comment\n";
        let held = HeldOutFunctions::hold_out(migration)?;
        assert_eq!((held.shielded(), held.restore(migration)?.as_str()), (migration, migration));
        Ok(())
    }
}
