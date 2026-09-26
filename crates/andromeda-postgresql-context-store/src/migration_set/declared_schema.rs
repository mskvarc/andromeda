//! The schema objects the embedded migrations declare, read back from their
//! text.
//!
//! The tests that hold the migrations to a rule -- a byte bound, a naming
//! convention, a storage declaration -- need the migration text and, for most
//! rules, the tables, columns, constraints, indexes and statistics objects that
//! text declares. This module reads both once, so every such test agrees on
//! what the migrations contain. The reader understands exactly the DDL shapes
//! the migrations use; a statement of any other shape is skipped.

use crate::migration_set::{platform::platform_migrations, tenant::tenant_migrator};
use sqlx::migrate::Migration;

/// Every embedded migration: platform, tenant and branch.
pub(super) fn every_migration() -> Vec<&'static Migration> {
    platform_migrations().iter().chain(tenant_migrator().migrations.iter()).collect()
}

/// The text of every embedded migration: platform, tenant and branch.
pub(super) fn every_migration_sql() -> Vec<&'static str> {
    every_migration().into_iter().map(|migration| migration.sql.as_str()).collect()
}

/// One column a `CREATE TABLE` or `ALTER TABLE ... ADD COLUMN` declares.
#[derive(Debug)]
pub(super) struct DeclaredColumn {
    /// The table the column belongs to.
    pub(super) table: String,
    /// The column name.
    pub(super) name: String,
    /// Everything after the name: the type and every column option.
    pub(super) definition: String,
}

impl DeclaredColumn {
    /// The first word of the type, upper-cased, with any array brackets kept.
    pub(super) fn type_word(&self) -> String {
        self.definition.split_whitespace().next().unwrap_or_default().to_uppercase()
    }

    /// Whether the column's values are variable-length and therefore stored
    /// through TOAST.
    pub(super) fn is_variable_length(&self) -> bool {
        let type_word = self.type_word();
        type_word.ends_with("[]") || ["TEXT", "JSONB", "NUMERIC", "BYTEA", "GEOMETRY", "GEOGRAPHY", "LTREE"].contains(&type_word.as_str())
    }

    /// Whether the column is an identity the database generates.
    pub(super) fn is_identity(&self) -> bool {
        self.definition.contains("AS IDENTITY")
    }
}

/// What a named constraint enforces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ConstraintKind {
    /// `PRIMARY KEY`.
    PrimaryKey,
    /// `UNIQUE`.
    Unique,
    /// `FOREIGN KEY`.
    ForeignKey,
    /// `CHECK`.
    Check,
}

/// One named constraint a table declares.
#[derive(Debug)]
pub(super) struct DeclaredConstraint {
    /// The table the constraint belongs to.
    pub(super) table: String,
    /// The constraint name.
    pub(super) name: String,
    /// What the constraint enforces.
    pub(super) kind: ConstraintKind,
    /// The column list a primary key, unique or foreign-key constraint names.
    pub(super) columns: Vec<String>,
    /// The referenced column list of a foreign key; empty for every other kind.
    pub(super) referenced_columns: Vec<String>,
}

/// What a named index-like object is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum IndexKind {
    /// `CREATE INDEX`.
    Plain,
    /// `CREATE UNIQUE INDEX`.
    Unique,
    /// `CREATE STATISTICS`.
    Statistics,
}

/// One index or extended-statistics object.
#[derive(Debug)]
pub(super) struct DeclaredIndex {
    /// The table the object is defined on.
    pub(super) table: String,
    /// The object name.
    pub(super) name: String,
    /// What the object is.
    pub(super) kind: IndexKind,
}

/// Every table, column, constraint, index and statistics object the embedded
/// migrations declare.
#[derive(Debug, Default)]
pub(super) struct DeclaredSchema {
    /// Every table, in declaration order.
    pub(super) tables: Vec<String>,
    /// Every column, in declaration order.
    pub(super) columns: Vec<DeclaredColumn>,
    /// Every named constraint.
    pub(super) constraints: Vec<DeclaredConstraint>,
    /// Every index and statistics object.
    pub(super) indexes: Vec<DeclaredIndex>,
}

impl DeclaredSchema {
    /// Reads the objects every embedded migration declares.
    pub(super) fn of_every_migration() -> DeclaredSchema {
        let mut schema = DeclaredSchema::default();
        for sql in every_migration_sql() {
            let text = without_comments(sql);
            text.split(';').for_each(|statement| schema.read_statement(statement.trim()));
        }
        schema
    }

    /// Records whatever objects `statement` declares.
    fn read_statement(&mut self, statement: &str) {
        if let Some(rest) = statement.strip_prefix("CREATE TABLE ") {
            self.read_create_table(rest);
        } else if let Some(rest) = statement.strip_prefix("ALTER TABLE ") {
            self.read_alter_table(rest);
        } else if let Some(rest) = statement.strip_prefix("CREATE UNIQUE INDEX ") {
            self.read_index(rest, IndexKind::Unique);
        } else if let Some(rest) = statement.strip_prefix("CREATE INDEX ") {
            self.read_index(rest, IndexKind::Plain);
        } else if let Some(rest) = statement.strip_prefix("CREATE STATISTICS ") {
            self.read_statistics(rest);
        }
    }

    /// Reads `<table> ( <items> ) [WITH (...)]`.
    fn read_create_table(&mut self, rest: &str) {
        let Some((table, body)) = rest.split_once('(') else {
            return;
        };
        let table = table.trim().to_owned();
        let body = &body[..closing_parenthesis(body)];
        for item in top_level_items(body) {
            match item.strip_prefix("CONSTRAINT ") {
                Some(constraint) => self.read_constraint(&table, constraint),
                None => self.read_column(&table, item),
            }
        }
        self.tables.push(table);
    }

    /// Reads `<table> ADD COLUMN ..., ADD CONSTRAINT ...`.
    fn read_alter_table(&mut self, rest: &str) {
        let Some((table, actions)) = rest.split_once(char::is_whitespace) else {
            return;
        };
        for action in top_level_items(actions) {
            if let Some(column) = action.strip_prefix("ADD COLUMN ") {
                self.read_column(table, column);
            } else if let Some(constraint) = action.strip_prefix("ADD CONSTRAINT ") {
                self.read_constraint(table, constraint);
            }
        }
    }

    /// Reads `<name> <definition>`.
    fn read_column(&mut self, table: &str, item: &str) {
        if let Some((name, definition)) = item.split_once(char::is_whitespace) {
            self.columns.push(DeclaredColumn {
                table: table.to_owned(),
                name: name.to_owned(),
                definition: definition.trim().to_owned(),
            });
        }
    }

    /// Reads `<name> PRIMARY KEY (...)`, `UNIQUE [NULLS NOT DISTINCT] (...)`,
    /// `FOREIGN KEY (...) REFERENCES <table> (...)` or `CHECK (...)`.
    fn read_constraint(&mut self, table: &str, constraint: &str) {
        let Some((name, definition)) = constraint.split_once(char::is_whitespace) else {
            return;
        };
        let definition = definition.trim();
        let kind = if definition.starts_with("PRIMARY KEY") {
            ConstraintKind::PrimaryKey
        } else if definition.starts_with("UNIQUE") {
            ConstraintKind::Unique
        } else if definition.starts_with("FOREIGN KEY") {
            ConstraintKind::ForeignKey
        } else if definition.starts_with("CHECK") {
            ConstraintKind::Check
        } else {
            return;
        };
        let (columns, referenced_columns) = match kind {
            ConstraintKind::PrimaryKey | ConstraintKind::Unique => (first_column_list(definition), Vec::new()),
            ConstraintKind::ForeignKey => match definition.split_once("REFERENCES") {
                Some((own, referenced)) => (first_column_list(own), first_column_list(referenced)),
                None => (first_column_list(definition), Vec::new()),
            },
            ConstraintKind::Check => (Vec::new(), Vec::new()),
        };
        self.constraints.push(DeclaredConstraint {
            table: table.to_owned(),
            name: name.to_owned(),
            kind,
            columns,
            referenced_columns,
        });
    }

    /// Reads `<name> ON <table> ...`.
    fn read_index(&mut self, rest: &str, kind: IndexKind) {
        let mut words = rest.split_whitespace();
        let (Some(name), Some("ON"), Some(table)) = (words.next(), words.next(), words.next()) else {
            return;
        };
        let table = table.split_once('(').map_or(table, |(table, _)| table);
        self.indexes.push(DeclaredIndex {
            table: table.to_owned(),
            name: name.to_owned(),
            kind,
        });
    }

    /// Reads `<name> (...) ON <columns> FROM <table>`.
    fn read_statistics(&mut self, rest: &str) {
        let name = rest.split_whitespace().next().unwrap_or_default();
        let table = rest.rsplit_once("FROM").map(|(_, table)| table.trim()).unwrap_or_default();
        if !name.is_empty() && !table.is_empty() {
            self.indexes.push(DeclaredIndex {
                table: table.to_owned(),
                name: name.to_owned(),
                kind: IndexKind::Statistics,
            });
        }
    }
}

/// `sql` with every `--` comment removed.
fn without_comments(sql: &str) -> String {
    sql.lines()
        .map(|line| line.split_once("--").map_or(line, |(code, _)| code))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The byte offset of the parenthesis that closes the one `text` follows.
fn closing_parenthesis(text: &str) -> usize {
    let mut depth = 0_usize;
    for (at, character) in text.char_indices() {
        if character == '(' {
            depth += 1;
        } else if character == ')' {
            if depth == 0 {
                return at;
            }
            depth -= 1;
        }
    }
    text.len()
}

/// `text` split on the commas that are not inside parentheses, each item
/// trimmed and the empty ones dropped.
fn top_level_items(text: &str) -> Vec<&str> {
    let mut items = Vec::new();
    let mut depth = 0_usize;
    let mut start = 0;
    for (at, character) in text.char_indices() {
        if character == '(' {
            depth += 1;
        } else if character == ')' {
            depth = depth.saturating_sub(1);
        } else if character == ',' && depth == 0 {
            items.push(text[start..at].trim());
            start = at + 1;
        }
    }
    items.push(text[start..].trim());
    items.into_iter().filter(|item| !item.is_empty()).collect()
}

/// The names in the first parenthesised list of `text`.
fn first_column_list(text: &str) -> Vec<String> {
    text.split_once('(')
        .and_then(|(_, rest)| rest.split_once(')'))
        .map(|(list, _)| list.split(',').map(|column| column.trim().to_owned()).collect())
        .unwrap_or_default()
}
