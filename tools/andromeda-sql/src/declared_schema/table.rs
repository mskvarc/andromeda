//! A table, its storage parameters, and its conversion to a hypertable.

use crate::declared_schema::origin::Origin;

/// One `create table`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeclaredTable {
    /// The table's name.
    name: String,
    /// Where its name is written.
    origin: Origin,
}

impl DeclaredTable {
    /// The table `name` at `origin`.
    #[must_use]
    pub fn new(name: impl Into<String>, origin: Origin) -> DeclaredTable {
        DeclaredTable { name: name.into(), origin }
    }

    /// The table's name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Where its name is written.
    #[must_use]
    pub const fn origin(&self) -> Origin {
        self.origin
    }
}

/// One storage parameter a `create table ... with (...)` sets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageParameter {
    /// The table.
    table: String,
    /// The parameter.
    name: String,
    /// Its value as written.
    value: String,
}

impl StorageParameter {
    /// `name = value` on `table`.
    #[must_use]
    pub fn new(table: impl Into<String>, name: impl Into<String>, value: impl Into<String>) -> StorageParameter {
        StorageParameter {
            table: table.into(),
            name: name.into(),
            value: value.into(),
        }
    }

    /// The table.
    #[must_use]
    pub fn table(&self) -> &str {
        &self.table
    }

    /// The parameter.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Its value as written.
    #[must_use]
    pub fn value(&self) -> &str {
        &self.value
    }
}

/// A table `create_hypertable` converts into a hypertable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hypertable {
    /// The table.
    table: String,
    /// The column it is partitioned on, when written as a string.
    partition_column: Option<String>,
    /// Where the conversion is written.
    origin: Origin,
}

impl Hypertable {
    /// The conversion of `table` on `partition_column` at `origin`.
    #[must_use]
    pub const fn new(table: String, partition_column: Option<String>, origin: Origin) -> Hypertable {
        Hypertable {
            table,
            partition_column,
            origin,
        }
    }

    /// The table.
    #[must_use]
    pub fn table(&self) -> &str {
        &self.table
    }

    /// The column it is partitioned on, when written as a string.
    #[must_use]
    pub fn partition_column(&self) -> Option<&str> {
        self.partition_column.as_deref()
    }

    /// Where the conversion is written.
    #[must_use]
    pub const fn origin(&self) -> Origin {
        self.origin
    }
}
