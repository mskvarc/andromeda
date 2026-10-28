//! An extended statistics object, as a migration declares it.

use crate::declared_schema::{origin::Origin, qualified_name::QualifiedName, statement_context::StatementContext};
use pg_query::{NodeEnum, protobuf::CreateStatsStmt};
use std::ops::Range;

/// One `create statistics`.
#[derive(Debug, Clone)]
pub struct DeclaredStatistics {
    /// The object's name.
    name: String,
    /// The table it gathers statistics of.
    table: String,
    /// Where its name is written.
    origin: Origin,
    /// The bytes of the migration holding its definition after its name.
    definition: Range<usize>,
}

impl DeclaredStatistics {
    /// The statistics object `statistics` declares, read from `statement`;
    /// `None` for one without a name or a table.
    #[must_use]
    pub fn read(statistics: &CreateStatsStmt, statement: &StatementContext<'_>) -> Option<DeclaredStatistics> {
        let name = QualifiedName::of_nodes(&statistics.defnames)?.name().to_owned();
        let table = statistics.relations.iter().find_map(|node| match &node.node {
            Some(NodeEnum::RangeVar(relation)) => Some(relation.relname.clone()),
            Some(_) | None => None,
        })?;
        let origin = statement.origin_of_name(&name);
        Some(DeclaredStatistics {
            definition: statement.rest_from(statement.after_name(origin.offset(), &name)),
            name,
            table,
            origin,
        })
    }

    /// The object's name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The table it gathers statistics of.
    #[must_use]
    pub fn table(&self) -> &str {
        &self.table
    }

    /// Where its name is written.
    #[must_use]
    pub const fn origin(&self) -> Origin {
        self.origin
    }

    /// The bytes of the migration holding its definition after its name.
    #[must_use]
    pub fn definition(&self) -> Range<usize> {
        self.definition.clone()
    }
}
