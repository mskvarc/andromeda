//! An anonymous code block, as a migration runs it.

use crate::{
    declared_schema::{
        function::{definition_element, string_argument},
        origin::Origin,
        statement_context::StatementContext,
    },
    plpgsql_tree::{PlpgsqlError, PlpgsqlRoutine},
};
use pg_query::protobuf::DoStmt;

/// The language a `do` block is written in when it names none.
const DEFAULT_LANGUAGE: &str = "plpgsql";

/// One `do` block.
#[derive(Debug)]
pub struct DoBlock {
    /// The language it is written in.
    language: String,
    /// Its PL/pgSQL tree, for a PL/pgSQL block.
    routines: Vec<PlpgsqlRoutine>,
    /// Where it is written.
    origin: Origin,
}

impl DoBlock {
    /// The block `block` runs, read from `statement`.
    ///
    /// # Errors
    ///
    /// Returns [`PlpgsqlError`] when a PL/pgSQL body cannot be read.
    pub fn read(block: &DoStmt, statement: &StatementContext<'_>) -> Result<DoBlock, PlpgsqlError> {
        let language = block
            .args
            .iter()
            .filter_map(definition_element)
            .find(|option| option.defname == "language")
            .and_then(string_argument)
            .unwrap_or(DEFAULT_LANGUAGE)
            .to_owned();
        let routines = if language == DEFAULT_LANGUAGE {
            PlpgsqlRoutine::parse(statement.text(), None)?
        } else {
            Vec::new()
        };
        Ok(DoBlock {
            language,
            routines,
            origin: statement.migration().origin(statement.span().start),
        })
    }

    /// The language it is written in.
    #[must_use]
    pub fn language(&self) -> &str {
        &self.language
    }

    /// Its PL/pgSQL tree, for a PL/pgSQL block.
    #[must_use]
    pub fn routines(&self) -> &[PlpgsqlRoutine] {
        &self.routines
    }

    /// Where it is written.
    #[must_use]
    pub const fn origin(&self) -> Origin {
        self.origin
    }
}
