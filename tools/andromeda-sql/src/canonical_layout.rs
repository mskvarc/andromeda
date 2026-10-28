//! The canonical layout of one migration: `pg_format` with the repository
//! configuration, function statements kept as written, one-line lists rejoined
//! and the file ending in one newline.

use crate::{
    file_ending::{with_final_newline, without_trailing_blank_lines},
    function_statement::{FunctionStatementError, HeldOutFunctions},
    list_rejoin::rejoin_one_line_lists,
    pg_format::{PgFormat, PgFormatError},
};

/// Lays a migration out canonically.
#[derive(Debug)]
pub struct CanonicalLayout {
    /// The formatter every file goes through.
    pg_format: PgFormat,
}

impl CanonicalLayout {
    /// The layout `pg_format` produces.
    #[must_use]
    pub const fn new(pg_format: PgFormat) -> CanonicalLayout {
        CanonicalLayout { pg_format }
    }

    /// The formatter every file goes through.
    #[must_use]
    pub const fn pg_format(&self) -> &PgFormat {
        &self.pg_format
    }

    /// The canonical text of the migration `sql`.
    ///
    /// The lists are rejoined before the function statements are put back, so
    /// a function statement keeps the text it was written with.
    ///
    /// # Errors
    ///
    /// Returns [`CanonicalLayoutError`] when the migration or the formatted
    /// text does not parse, `pg_format` fails, or the function statements do
    /// not come back as held out.
    pub fn of(&self, sql: &str) -> Result<String, CanonicalLayoutError> {
        let held = HeldOutFunctions::hold_out(sql)?;
        let formatted = self.pg_format.format(&with_final_newline(held.shielded()))?;
        let restored = held.restore(&rejoin_one_line_lists(&formatted))?;
        Ok(without_trailing_blank_lines(&restored))
    }
}

/// Why a migration could not be laid out.
#[derive(Debug, thiserror::Error)]
pub enum CanonicalLayoutError {
    /// `pg_format` failed on the migration.
    #[error(transparent)]
    PgFormat(#[from] PgFormatError),
    /// The function statements could not be held out or put back.
    #[error(transparent)]
    FunctionStatement(#[from] FunctionStatementError),
}
