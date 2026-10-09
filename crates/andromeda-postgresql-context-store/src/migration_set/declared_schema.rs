//! The schema objects the embedded migrations declare, read back from their
//! text.
//!
//! The tests that hold the migrations to a rule (a byte bound, a naming
//! convention, a storage declaration) need the migration text and, for most
//! rules, the tables, columns, constraints, indexes, statistics objects,
//! functions and triggers that text declares. This module reads both once, so every such test agrees on
//! what the migrations contain. The reader understands exactly the DDL shapes
//! the migrations use; a statement of any other shape is skipped.

use crate::migration_set::{
    ddl_text::{
        closing_parenthesis,
        contains_ci,
        find_ci,
        first_column_list,
        rfind_ci,
        starts_with_ci,
        statements,
        strip_prefix_ci,
        top_level_items,
        without_comments,
    },
    platform::platform_migrations,
    tenant::tenant_migrator,
};
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
        contains_ci(&self.definition, "AS IDENTITY")
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
    /// Everything after the name, from the constraint kind on.
    pub(super) definition: String,
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
    /// Everything after the name: for an index, the table, method, keys,
    /// storage parameters and predicate; for a statistics object, its kinds,
    /// expressions and table.
    pub(super) definition: String,
}

/// One `CREATE [OR REPLACE] FUNCTION`.
#[derive(Debug)]
pub(super) struct DeclaredFunction {
    /// The function name.
    pub(super) name: String,
    /// Everything after the name: the arguments, the return type, every
    /// attribute and the body.
    pub(super) definition: String,
}

/// One `CREATE TRIGGER`.
#[derive(Debug)]
pub(super) struct DeclaredTrigger {
    /// The table the trigger fires on.
    pub(super) table: String,
    /// The trigger name.
    pub(super) name: String,
    /// Everything after the name: timing, event, transition tables, level and
    /// the function executed.
    pub(super) definition: String,
}

/// Every table, column, constraint, index, statistics object, function and
/// trigger the embedded migrations declare.
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
    /// Every function.
    pub(super) functions: Vec<DeclaredFunction>,
    /// Every trigger.
    pub(super) triggers: Vec<DeclaredTrigger>,
}

impl DeclaredSchema {
    /// Reads the objects every embedded migration declares.
    pub(super) fn of_every_migration() -> DeclaredSchema {
        let mut schema = DeclaredSchema::default();
        for sql in every_migration_sql() {
            let text = without_comments(sql);
            for statement in statements(&text) {
                schema.read_statement(statement);
            }
        }
        schema
    }

    /// Records whatever objects `statement` declares.
    fn read_statement(&mut self, statement: &str) {
        if let Some(rest) = strip_prefix_ci(statement, "CREATE TABLE ") {
            self.read_create_table(rest);
        } else if let Some(rest) = strip_prefix_ci(statement, "ALTER TABLE ") {
            self.read_alter_table(rest);
        } else if let Some(rest) = strip_prefix_ci(statement, "CREATE UNIQUE INDEX ") {
            self.read_index(rest, IndexKind::Unique);
        } else if let Some(rest) = strip_prefix_ci(statement, "CREATE INDEX ") {
            self.read_index(rest, IndexKind::Plain);
        } else if let Some(rest) = strip_prefix_ci(statement, "CREATE STATISTICS ") {
            self.read_statistics(rest);
        } else if let Some(rest) = strip_prefix_ci(statement, "CREATE FUNCTION ").or_else(|| strip_prefix_ci(statement, "CREATE OR REPLACE FUNCTION ")) {
            self.read_function(rest);
        } else if let Some(rest) = strip_prefix_ci(statement, "CREATE TRIGGER ") {
            self.read_trigger(rest);
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
            match strip_prefix_ci(item, "CONSTRAINT ") {
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
            if let Some(column) = strip_prefix_ci(action, "ADD COLUMN ") {
                self.read_column(table, column);
            } else if let Some(constraint) = strip_prefix_ci(action, "ADD CONSTRAINT ") {
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
        let kind = if starts_with_ci(definition, "PRIMARY KEY") {
            ConstraintKind::PrimaryKey
        } else if starts_with_ci(definition, "UNIQUE") {
            ConstraintKind::Unique
        } else if starts_with_ci(definition, "FOREIGN KEY") {
            ConstraintKind::ForeignKey
        } else if starts_with_ci(definition, "CHECK") {
            ConstraintKind::Check
        } else {
            return;
        };
        let (columns, referenced_columns) = match kind {
            ConstraintKind::PrimaryKey | ConstraintKind::Unique => (first_column_list(definition), Vec::new()),
            ConstraintKind::ForeignKey => match find_ci(definition, "REFERENCES") {
                Some(at) => (first_column_list(&definition[..at]), first_column_list(&definition[at + "REFERENCES".len()..])),
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
            definition: definition.to_owned(),
        });
    }

    /// Reads `<name> ON <table> ...`.
    fn read_index(&mut self, rest: &str, kind: IndexKind) {
        let mut words = rest.split_whitespace();
        let (Some(name), Some(on), Some(table)) = (words.next(), words.next(), words.next()) else {
            return;
        };
        if !on.eq_ignore_ascii_case("ON") {
            return;
        }
        let table = table.split_once('(').map_or(table, |(table, _)| table);
        self.indexes.push(DeclaredIndex {
            table: table.to_owned(),
            name: name.to_owned(),
            kind,
            definition: rest[name.len()..].trim().to_owned(),
        });
    }

    /// Reads `<name> (...) ON <columns> FROM <table>`.
    fn read_statistics(&mut self, rest: &str) {
        let name = rest.split_whitespace().next().unwrap_or_default();
        let table = rfind_ci(rest, "FROM").map(|at| rest[at + "FROM".len()..].trim()).unwrap_or_default();
        if !name.is_empty() && !table.is_empty() {
            self.indexes.push(DeclaredIndex {
                table: table.to_owned(),
                name: name.to_owned(),
                kind: IndexKind::Statistics,
                definition: rest[name.len()..].trim().to_owned(),
            });
        }
    }

    /// Reads `<name> (<arguments>) RETURNS ... AS $$ <body> $$`.
    fn read_function(&mut self, rest: &str) {
        let Some((name, definition)) = rest.split_once('(') else {
            return;
        };
        self.functions.push(DeclaredFunction {
            name: name.trim().to_owned(),
            definition: format!("({definition}"),
        });
    }

    /// Reads `<name> <timing> <event> ON <table> ...`.
    fn read_trigger(&mut self, rest: &str) {
        let mut words = rest.split_whitespace();
        let Some(name) = words.next() else {
            return;
        };
        let Some(table) = words.skip_while(|word| !word.eq_ignore_ascii_case("ON")).nth(1) else {
            return;
        };
        self.triggers.push(DeclaredTrigger {
            table: table.to_owned(),
            name: name.to_owned(),
            definition: rest.trim_start()[name.len()..].trim().to_owned(),
        });
    }
}

#[cfg(test)]
mod tests {
    use crate::migration_set::declared_schema::DeclaredSchema;
    use pretty_assertions::assert_eq;

    #[test]
    fn a_trigger_is_read_with_the_table_it_fires_on() {
        let mut schema = DeclaredSchema::default();
        schema.read_statement(
            "create trigger trg_t_check_insert\n    after insert on t referencing new table as new_rows for each statement\n    execute function check_t ()",
        );
        let [trigger] = schema.triggers.as_slice() else {
            panic!("one trigger is read: {:?}", schema.triggers);
        };
        assert_eq!((trigger.table.as_str(), trigger.name.as_str()), ("t", "trg_t_check_insert"));
        assert!(
            trigger.definition.starts_with("after insert on t referencing new table"),
            "{}",
            trigger.definition
        );
    }

    #[test]
    fn a_function_is_read_with_its_attributes_and_body() {
        let mut schema = DeclaredSchema::default();
        schema.read_statement("create or replace function f (v integer)\n    returns integer\n    language sql\n    immutable strict parallel safe\n    as $$\n    select\n        v\n$$");
        let [function] = schema.functions.as_slice() else {
            panic!("one function is read: {:?}", schema.functions);
        };
        assert_eq!(function.name, "f");
        assert!(function.definition.starts_with("(v integer)"), "{}", function.definition);
        assert!(function.definition.contains("immutable strict parallel safe"), "{}", function.definition);
    }
}
