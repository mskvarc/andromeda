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
pub(crate) fn every_migration() -> Vec<&'static Migration> {
    platform_migrations().iter().chain(tenant_migrator().migrations.iter()).collect()
}

/// The text of every embedded migration: platform, tenant and branch.
pub(crate) fn every_migration_sql() -> Vec<&'static str> {
    every_migration().into_iter().map(|migration| migration.sql.as_str()).collect()
}

/// One column a `CREATE TABLE` or `ALTER TABLE ... ADD COLUMN` declares.
#[derive(Debug)]
pub(crate) struct DeclaredColumn {
    /// The table the column belongs to.
    pub(crate) table: String,
    /// The column name.
    pub(crate) name: String,
    /// Everything after the name: the type and every column option.
    pub(crate) definition: String,
}

impl DeclaredColumn {
    /// The first word of the type, upper-cased, with any array brackets kept.
    pub(crate) fn type_word(&self) -> String {
        self.definition.split_whitespace().next().unwrap_or_default().to_uppercase()
    }

    /// Whether the column's values are variable-length and therefore stored
    /// through TOAST.
    pub(crate) fn is_variable_length(&self) -> bool {
        let type_word = self.type_word();
        type_word.ends_with("[]") || ["TEXT", "JSON", "JSONB", "NUMERIC", "BYTEA", "GEOMETRY", "GEOGRAPHY", "LTREE"].contains(&type_word.as_str())
    }

    /// Whether the column is an identity the database generates.
    pub(crate) fn is_identity(&self) -> bool {
        contains_ci(&self.definition, "AS IDENTITY")
    }
}

/// What a named constraint enforces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConstraintKind {
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
pub(crate) struct DeclaredConstraint {
    /// The table the constraint belongs to.
    pub(crate) table: String,
    /// The constraint name.
    pub(crate) name: String,
    /// What the constraint enforces.
    pub(crate) kind: ConstraintKind,
    /// The column list a primary key, unique or foreign-key constraint names.
    pub(crate) columns: Vec<String>,
    /// The referenced column list of a foreign key; empty for every other kind.
    pub(crate) referenced_columns: Vec<String>,
    /// Everything after the name, from the constraint kind on.
    pub(crate) definition: String,
}

/// What a named index-like object is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IndexKind {
    /// `CREATE INDEX`.
    Plain,
    /// `CREATE UNIQUE INDEX`.
    Unique,
    /// `CREATE STATISTICS`.
    Statistics,
}

/// One index or extended-statistics object.
#[derive(Debug)]
pub(crate) struct DeclaredIndex {
    /// The table the object is defined on.
    pub(crate) table: String,
    /// The object name.
    pub(crate) name: String,
    /// What the object is.
    pub(crate) kind: IndexKind,
    /// Everything after the name: for an index, the table, method, keys,
    /// storage parameters and predicate; for a statistics object, its kinds,
    /// expressions and table.
    pub(crate) definition: String,
}

/// One storage parameter a `CREATE TABLE ... WITH (...)` sets.
#[derive(Debug)]
pub(crate) struct DeclaredStorageParameter {
    /// The table the parameter is set on.
    pub(crate) table: String,
    /// The parameter name.
    pub(crate) name: String,
    /// The value as written.
    pub(crate) value: String,
}

/// One `CREATE [OR REPLACE] FUNCTION`.
#[derive(Debug)]
pub(crate) struct DeclaredFunction {
    /// The function name.
    pub(crate) name: String,
    /// Everything after the name: the arguments, the return type, every
    /// attribute and the body.
    pub(crate) definition: String,
}

/// One `CREATE TRIGGER`.
#[derive(Debug)]
pub(crate) struct DeclaredTrigger {
    /// The table the trigger fires on.
    pub(crate) table: String,
    /// The trigger name.
    pub(crate) name: String,
    /// Everything after the name: timing, event, transition tables, level and
    /// the function executed.
    pub(crate) definition: String,
}

/// Every table, column, composite type field, constraint, index, statistics
/// object, function and trigger the embedded migrations declare.
#[derive(Debug, Default)]
pub(crate) struct DeclaredSchema {
    /// Every table, in declaration order.
    pub(crate) tables: Vec<String>,
    /// Every column, in declaration order.
    pub(crate) columns: Vec<DeclaredColumn>,
    /// Every field of a composite type, in declaration order, each named with
    /// its type in place of a table. A field declares no storage of its own:
    /// the column of the composite type stores it.
    pub(crate) composite_fields: Vec<DeclaredColumn>,
    /// Every named constraint.
    pub(crate) constraints: Vec<DeclaredConstraint>,
    /// Every table storage parameter, in declaration order.
    pub(crate) storage_parameters: Vec<DeclaredStorageParameter>,
    /// Every index and statistics object.
    pub(crate) indexes: Vec<DeclaredIndex>,
    /// Every function.
    pub(crate) functions: Vec<DeclaredFunction>,
    /// Every trigger.
    pub(crate) triggers: Vec<DeclaredTrigger>,
}

impl DeclaredSchema {
    /// Reads the objects every embedded migration declares.
    pub(crate) fn of_every_migration() -> DeclaredSchema {
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
        } else if let Some(rest) = strip_prefix_ci(statement, "CREATE TYPE ") {
            self.read_composite_type(rest);
        }
    }

    /// Reads `<type> AS ( <fields> )`.
    fn read_composite_type(&mut self, rest: &str) {
        let Some((head, body)) = rest.split_once('(') else {
            return;
        };
        let Some(type_name) = head.split_whitespace().next() else {
            return;
        };
        let body = &body[..closing_parenthesis(body)];
        for field in top_level_items(body) {
            if let Some((name, definition)) = field.split_once(char::is_whitespace) {
                self.composite_fields.push(DeclaredColumn {
                    table: type_name.to_owned(),
                    name: name.to_owned(),
                    definition: definition.trim().to_owned(),
                });
            }
        }
    }

    /// Reads `<table> ( <items> ) [WITH (...)]`.
    fn read_create_table(&mut self, rest: &str) {
        let Some((table, body)) = rest.split_once('(') else {
            return;
        };
        let table = table.trim().to_owned();
        let body_end = closing_parenthesis(body);
        for item in top_level_items(&body[..body_end]) {
            match strip_prefix_ci(item, "CONSTRAINT ") {
                Some(constraint) => self.read_constraint(&table, constraint),
                None => self.read_column(&table, item),
            }
        }
        let after_body = body.get(body_end + 1..).unwrap_or_default().trim_start();
        if let Some(parameters) = strip_prefix_ci(after_body, "WITH").map(str::trim_start).and_then(|rest| rest.strip_prefix('(')) {
            for parameter in top_level_items(&parameters[..closing_parenthesis(parameters)]) {
                if let Some((name, value)) = parameter.split_once('=') {
                    self.storage_parameters.push(DeclaredStorageParameter {
                        table: table.clone(),
                        name: name.trim().to_owned(),
                        value: value.trim().to_owned(),
                    });
                }
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
#[expect(
    clippy::panic,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::migration_set::declared_schema::{DeclaredColumn, DeclaredSchema};
    use pretty_assertions::assert_eq;

    #[test]
    fn a_table_is_read_with_its_storage_parameters() {
        let mut schema = DeclaredSchema::default();
        schema.read_statement("create table t (\n    id bigint not null\n)\nwith (\n    fillfactor = 80,\n    vacuum_truncate = false\n)");
        let parameters: Vec<(&str, &str, &str)> = schema
            .storage_parameters
            .iter()
            .map(|parameter| (parameter.table.as_str(), parameter.name.as_str(), parameter.value.as_str()))
            .collect();
        assert_eq!(parameters, [("t", "fillfactor", "80"), ("t", "vacuum_truncate", "false")]);
        assert_eq!(schema.tables, ["t"]);
    }

    #[test]
    fn a_composite_type_is_read_field_by_field_under_its_name() {
        let mut schema = DeclaredSchema::default();
        schema.read_statement("create type pair as (\n    left_value integer,\n    right_value jsonb\n)");
        let fields: Vec<(&str, &str, &str)> = schema
            .composite_fields
            .iter()
            .map(|field| (field.table.as_str(), field.name.as_str(), field.definition.as_str()))
            .collect();
        assert_eq!(fields, [("pair", "left_value", "integer"), ("pair", "right_value", "jsonb")]);
        assert!(schema.columns.is_empty(), "a composite field is no table column");
    }

    #[test]
    fn json_is_a_variable_length_type() {
        let column = DeclaredColumn {
            table: "jsonld_contexts".to_owned(),
            name: "payload".to_owned(),
            definition: "json compression lz4 not null".to_owned(),
        };
        assert!(column.is_variable_length());
    }

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
