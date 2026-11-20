//! A trigger, as a migration declares it.

use crate::declared_schema::{origin::Origin, qualified_name::QualifiedName, statement_context::StatementContext};
use pg_query::{NodeEnum, protobuf::CreateTrigStmt};
use std::ops::Range;

/// One `create trigger`.
#[derive(Debug, Clone)]
pub struct DeclaredTrigger {
    /// The trigger's name.
    name: String,
    /// The table it fires on.
    table: String,
    /// The function it executes.
    function: QualifiedName,
    /// The names it gives its transition tables.
    transition_tables: Vec<String>,
    /// Where its name is written.
    origin: Origin,
    /// The bytes of the migration holding its definition after its name.
    definition: Range<usize>,
}

impl DeclaredTrigger {
    /// The trigger `trigger` declares, read from `statement`; `None` for one
    /// without a function.
    #[must_use]
    pub fn read(trigger: &CreateTrigStmt, statement: &StatementContext<'_>) -> Option<DeclaredTrigger> {
        let function = QualifiedName::of_nodes(&trigger.funcname)?;
        let origin = statement.origin_of_name(&trigger.trigname);
        let after_name = statement.after_name(origin.offset(), &trigger.trigname);
        Some(DeclaredTrigger {
            name: trigger.trigname.clone(),
            table: trigger.relation.as_ref().map_or_default(|relation| relation.relname.clone()),
            transition_tables: trigger
                .transition_rels
                .iter()
                .filter_map(|node| match &node.node {
                    Some(NodeEnum::TriggerTransition(transition)) => Some(transition.name.clone()),
                    Some(_) | None => None,
                })
                .collect(),
            function,
            origin,
            definition: statement.rest_from(after_name),
        })
    }

    /// The trigger's name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The table it fires on.
    #[must_use]
    pub fn table(&self) -> &str {
        &self.table
    }

    /// The function it executes.
    #[must_use]
    pub const fn function(&self) -> &QualifiedName {
        &self.function
    }

    /// The names it gives its transition tables.
    #[must_use]
    pub fn transition_tables(&self) -> &[String] {
        &self.transition_tables
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
