//! A constraint of a table, as a migration declares it.

use crate::declared_schema::{origin::Origin, qualified_name::QualifiedName, statement_context::StatementContext};
use pg_query::{
    Node,
    NodeEnum,
    protobuf::{ConstrType, Constraint},
};
use std::ops::Range;

/// What a constraint enforces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::Display)]
#[strum(serialize_all = "kebab-case")]
pub enum ConstraintKind {
    /// `primary key`.
    PrimaryKey,
    /// `unique`.
    Unique,
    /// `foreign key ... references`.
    ForeignKey,
    /// `check`.
    Check,
    /// `exclude`.
    Exclusion,
}

/// One constraint of a table.
#[derive(Debug, Clone)]
pub struct DeclaredConstraint {
    /// The table the constraint belongs to.
    table: String,
    /// Its name, absent for one the database names.
    name: Option<String>,
    /// What it enforces.
    kind: ConstraintKind,
    /// The columns a key, a unique constraint or a foreign key covers, in order.
    columns: Vec<String>,
    /// The table a foreign key references.
    referenced_table: Option<QualifiedName>,
    /// The columns a foreign key references, in order.
    referenced_columns: Vec<String>,
    /// The expression a check tests.
    expression: Option<Node>,
    /// Whether it is added `not valid`, checked only for rows written later.
    not_valid: bool,
    /// Where it is written.
    origin: Origin,
    /// The bytes of the migration holding its definition after its name.
    definition: Range<usize>,
}

impl DeclaredConstraint {
    /// The constraint `constraint` declares on `table`, read from `statement`;
    /// `column` is the column a column constraint is written on. `None` for a
    /// constraint kind that is a column property instead (`not null`, a
    /// default, an identity, a generated value).
    #[must_use]
    pub fn read(constraint: &Constraint, table: &str, column: Option<&str>, statement: &StatementContext<'_>) -> Option<DeclaredConstraint> {
        let kind = match ConstrType::try_from(constraint.contype).ok()? {
            ConstrType::ConstrPrimary => ConstraintKind::PrimaryKey,
            ConstrType::ConstrUnique => ConstraintKind::Unique,
            ConstrType::ConstrForeign => ConstraintKind::ForeignKey,
            ConstrType::ConstrCheck => ConstraintKind::Check,
            ConstrType::ConstrExclusion => ConstraintKind::Exclusion,
            ConstrType::Undefined
            | ConstrType::ConstrNull
            | ConstrType::ConstrNotnull
            | ConstrType::ConstrDefault
            | ConstrType::ConstrIdentity
            | ConstrType::ConstrGenerated
            | ConstrType::ConstrAttrDeferrable
            | ConstrType::ConstrAttrNotDeferrable
            | ConstrType::ConstrAttrDeferred
            | ConstrType::ConstrAttrImmediate
            | ConstrType::ConstrAttrEnforced
            | ConstrType::ConstrAttrNotEnforced => return None,
        };
        let listed = |nodes: &[Node]| strings(nodes);
        let columns = match kind {
            ConstraintKind::ForeignKey => listed(&constraint.fk_attrs),
            ConstraintKind::PrimaryKey | ConstraintKind::Unique | ConstraintKind::Exclusion => listed(&constraint.keys),
            ConstraintKind::Check => Vec::new(),
        };
        let columns = match (columns.is_empty(), column, kind) {
            (true, Some(column), ConstraintKind::PrimaryKey | ConstraintKind::Unique | ConstraintKind::ForeignKey) => vec![column.to_owned()],
            (_, _, _) => columns,
        };
        let name = Some(constraint.conname.as_str()).filter(|name| !name.is_empty()).map(str::to_owned);
        let origin = statement.origin_at(constraint.location);
        let definition_start = name.as_deref().map_or(origin.offset(), |name| statement.after_name(origin.offset(), name));
        Some(DeclaredConstraint {
            table: table.to_owned(),
            kind,
            columns,
            referenced_table: constraint.pktable.as_ref().map(QualifiedName::of_relation),
            referenced_columns: listed(&constraint.pk_attrs),
            // clone: the constraint keeps its expression beyond the statement it is read from
            expression: constraint.raw_expr.as_deref().cloned(),
            not_valid: constraint.skip_validation,
            definition: statement.item_from(definition_start),
            origin,
            name,
        })
    }

    /// The table the constraint belongs to.
    #[must_use]
    pub fn table(&self) -> &str {
        &self.table
    }

    /// Its name, absent for one the database names.
    #[must_use]
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    /// What it enforces.
    #[must_use]
    pub const fn kind(&self) -> ConstraintKind {
        self.kind
    }

    /// The columns a key, a unique constraint or a foreign key covers, in order.
    #[must_use]
    pub fn columns(&self) -> &[String] {
        &self.columns
    }

    /// The table a foreign key references.
    #[must_use]
    pub const fn referenced_table(&self) -> Option<&QualifiedName> {
        self.referenced_table.as_ref()
    }

    /// The columns a foreign key references, in order.
    #[must_use]
    pub fn referenced_columns(&self) -> &[String] {
        &self.referenced_columns
    }

    /// The expression a check tests.
    #[must_use]
    pub const fn expression(&self) -> Option<&Node> {
        self.expression.as_ref()
    }

    /// Whether it is added `not valid`, checked only for rows written later.
    #[must_use]
    pub const fn is_not_valid(&self) -> bool {
        self.not_valid
    }

    /// Where it is written.
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

/// The strings of a list of string nodes.
fn strings(nodes: &[Node]) -> Vec<String> {
    nodes
        .iter()
        .filter_map(|node| match &node.node {
            Some(NodeEnum::String(string)) => Some(string.sval.clone()),
            Some(_) | None => None,
        })
        .collect()
}

/// An `alter table ... validate constraint`, which checks the existing rows
/// against a constraint added `not valid`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConstraintValidation {
    /// The table.
    table: String,
    /// The constraint validated.
    name: String,
    /// Where the constraint's name is written.
    origin: Origin,
}

impl ConstraintValidation {
    /// The validation of `name` on `table` at `origin`.
    #[must_use]
    pub fn new(table: &str, name: &str, origin: Origin) -> ConstraintValidation {
        ConstraintValidation {
            table: table.to_owned(),
            name: name.to_owned(),
            origin,
        }
    }

    /// The table.
    #[must_use]
    pub fn table(&self) -> &str {
        &self.table
    }

    /// The constraint validated.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Where the constraint's name is written.
    #[must_use]
    pub const fn origin(&self) -> Origin {
        self.origin
    }
}
