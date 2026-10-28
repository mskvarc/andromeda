//! The statement a declared object is read from, and where in its migration
//! that statement is.

use crate::declared_schema::{origin::Origin, parsed_migration::ParsedMigration};
use std::ops::Range;

/// One statement of a migration, as its objects are read.
#[derive(Debug, Clone, Copy)]
pub struct StatementContext<'migration> {
    /// The migration.
    migration: &'migration ParsedMigration,
    /// The bytes of the migration the statement covers.
    start: usize,
    /// Where the statement ends, its semicolon included.
    end: usize,
}

impl<'migration> StatementContext<'migration> {
    /// The statement of `migration` covering `span`.
    #[must_use]
    pub const fn new(migration: &'migration ParsedMigration, span: Range<usize>) -> StatementContext<'migration> {
        StatementContext {
            migration,
            start: span.start,
            end: span.end,
        }
    }

    /// The migration.
    #[must_use]
    pub const fn migration(&self) -> &'migration ParsedMigration {
        self.migration
    }

    /// The bytes of the migration the statement covers.
    #[must_use]
    pub const fn span(&self) -> Range<usize> {
        self.start..self.end
    }

    /// The statement's text.
    #[must_use]
    pub fn text(&self) -> &'migration str {
        &self.migration.text()[self.span()]
    }

    /// Where the parser location `location` is, or the statement's start when
    /// the parser does not know it.
    #[must_use]
    pub fn origin_at(&self, location: i32) -> Origin {
        self.migration
            .origin(usize::try_from(location).map_or(self.start, |offset| offset.max(self.start)))
    }

    /// Where the first token spelling `name` is, or the statement's start.
    #[must_use]
    pub fn origin_of_name(&self, name: &str) -> Origin {
        self.migration.origin(self.migration.name_offset(self.span(), name).unwrap_or(self.start))
    }

    /// The bytes from `start` to the end of its list item: the definition of
    /// a column or a constraint after its name.
    #[must_use]
    pub fn item_from(&self, start: usize) -> Range<usize> {
        let start = start.min(self.end);
        start..self.migration.item_end(start, self.span())
    }

    /// The bytes from `start` to the end of the statement, its semicolon left
    /// out.
    #[must_use]
    pub fn rest_from(&self, start: usize) -> Range<usize> {
        let start = start.min(self.end);
        let end = if self.migration.text().as_bytes().get(self.end.saturating_sub(1)) == Some(&b';') {
            self.end - 1
        } else {
            self.end
        };
        start..end.max(start)
    }

    /// The offset just past the first token spelling `name` at or after
    /// `start`, or `start` when there is none.
    #[must_use]
    pub fn after_name(&self, start: usize, name: &str) -> usize {
        self.migration.name_offset(start.min(self.end)..self.end, name).map_or(start, |offset| {
            self.migration
                .tokens()
                .iter()
                .find(|token| token.range().start == offset)
                .map_or(offset, |token| token.range().end)
        })
    }
}
