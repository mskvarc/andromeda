//! The top-level statements of an SQL text, with the bytes each one covers.
//!
//! Statement boundaries are the ones the PostgreSQL parser reports: a
//! statement runs from its first token, comments before it excluded, to the
//! semicolon that ends it.

use pg_query::{
    NodeEnum,
    protobuf::{RawStmt, ScanToken, Token},
};
use std::{num::TryFromIntError, ops::Range};

/// One top-level statement.
#[derive(Debug)]
pub struct SqlStatement {
    /// The bytes of the text the statement covers, its semicolon included.
    span: Range<usize>,
    /// The statement's syntax tree, absent for an empty statement.
    node: Option<NodeEnum>,
}

impl SqlStatement {
    /// Every top-level statement of `sql`, in order, with `tokens`, the scan of
    /// `sql`, locating where each one starts.
    ///
    /// # Errors
    ///
    /// Returns [`SqlStatementError`] when `sql` does not parse, or the parser
    /// reports an offset outside it.
    pub fn parse_all(sql: &str, tokens: &[ScanToken]) -> Result<Vec<SqlStatement>, SqlStatementError> {
        pg_query::parse(sql, 0)
            .map_err(SqlStatementError::Unparsed)?
            .protobuf
            .stmts
            .into_iter()
            .map(|statement| {
                Ok(SqlStatement {
                    span: statement_span(sql, &statement, tokens)?,
                    node: statement.stmt.and_then(|node| node.node),
                })
            })
            .collect()
    }

    /// The bytes of the text the statement covers, its semicolon included.
    #[must_use]
    pub fn span(&self) -> Range<usize> {
        self.span.clone()
    }

    /// The statement's syntax tree, absent for an empty statement.
    #[must_use]
    pub const fn node(&self) -> Option<&NodeEnum> {
        self.node.as_ref()
    }
}

/// The bytes of `sql` the statement covers, from its first token to its
/// terminating semicolon when it has one.
fn statement_span(sql: &str, statement: &RawStmt, tokens: &[ScanToken]) -> Result<Range<usize>, SqlStatementError> {
    let location = usize::try_from(statement.stmt_location).map_err(SqlStatementError::Offset)?;
    // A length of zero is how the parser marks a statement that runs to the end
    // of the text.
    let end = match usize::try_from(statement.stmt_len).map_err(SqlStatementError::Offset)? {
        0 => sql.len(),
        length => location + length,
    };
    let start = tokens
        .iter()
        .filter(|token| !matches!(Token::try_from(token.token), Ok(Token::SqlComment | Token::CComment)))
        .filter_map(|token| usize::try_from(token.start).ok())
        .find(|start| *start >= location)
        .filter(|start| *start < end)
        .unwrap_or(location);
    let end = if sql.as_bytes().get(end) == Some(&b';') { end + 1 } else { end };
    Ok(start..end)
}

/// Why the statements of a text could not be read.
#[derive(Debug, thiserror::Error)]
pub enum SqlStatementError {
    /// The text does not parse as PostgreSQL.
    #[error("it does not parse: {0}")]
    Unparsed(pg_query::Error),
    /// The parser reported a statement offset outside the text.
    #[error("the parser reported a negative statement offset: {0}")]
    Offset(TryFromIntError),
}

#[cfg(test)]
mod tests {
    use crate::sql_statement::{SqlStatement, SqlStatementError};
    use pretty_assertions::assert_eq;

    #[test]
    fn a_statement_starts_after_its_comment_and_ends_with_its_semicolon() -> Result<(), SqlStatementError> {
        let sql = "-- Holds entities.\ncreate table t (a integer);\n\nselect 1";
        let tokens = pg_query::scan(sql).map_err(SqlStatementError::Unparsed)?.tokens;
        let spans: Vec<&str> = SqlStatement::parse_all(sql, &tokens)?.iter().map(|statement| &sql[statement.span()]).collect();
        assert_eq!(spans, ["create table t (a integer);", "select 1"]);
        Ok(())
    }
}
