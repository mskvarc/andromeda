//! A column of a table or a field of a composite type, as a migration declares
//! it.

use crate::declared_schema::{declared_type::DeclaredType, origin::Origin, qualified_name::QualifiedName, statement_context::StatementContext};
use pg_query::{
    NodeEnum,
    protobuf::{ColumnDef, ConstrType},
};
use std::ops::Range;

/// The statement that declares a column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColumnSource {
    /// A column of a `create table`.
    CreateTable,
    /// A column `alter table ... add column` adds.
    AddColumn,
    /// The new type `alter table ... alter column ... type` gives a column.
    AlterColumnType,
    /// A field of a `create type ... as (...)`.
    CompositeType,
}

/// What a column declares.
#[derive(Debug, Clone)]
pub struct DeclaredColumn {
    /// The table or composite type the column belongs to.
    relation: String,
    /// The column's name.
    name: String,
    /// Its type.
    data_type: DeclaredType,
    /// The compression method it names, if any.
    compression: Option<String>,
    /// The collation it names, if any.
    collation: Option<QualifiedName>,
    /// Whether the database generates its values as an identity.
    identity: bool,
    /// Whether it is a stored generated column.
    generated: bool,
    /// Whether it is declared `not null` or is a primary key column.
    not_null: bool,
    /// The statement that declares it.
    source: ColumnSource,
    /// Where its name is written.
    origin: Origin,
    /// The bytes of the migration holding its definition after its name.
    definition: Range<usize>,
}

impl DeclaredColumn {
    /// The column `name` that `definition` declares in `relation`, read from
    /// `statement`; `None` for a definition without a type. The name is given
    /// apart because a retyping names its column outside its definition.
    #[must_use]
    pub fn read(definition: &ColumnDef, name: &str, relation: &str, source: ColumnSource, statement: &StatementContext<'_>) -> Option<DeclaredColumn> {
        let constraint_kinds = definition
            .constraints
            .iter()
            .filter_map(|node| match &node.node {
                Some(NodeEnum::Constraint(constraint)) => ConstrType::try_from(constraint.contype).ok(),
                Some(_) | None => None,
            })
            .collect::<Vec<_>>();
        let origin = if definition.location < 0 {
            statement.origin_of_name(name)
        } else {
            statement.origin_at(definition.location)
        };
        Some(DeclaredColumn {
            relation: relation.to_owned(),
            name: name.to_owned(),
            data_type: DeclaredType::of(definition.type_name.as_ref()?)?,
            compression: Some(definition.compression.as_str()).filter(|method| !method.is_empty()).map(str::to_owned),
            collation: definition.coll_clause.as_ref().and_then(|clause| QualifiedName::of_nodes(&clause.collname)),
            identity: constraint_kinds.contains(&ConstrType::ConstrIdentity),
            generated: constraint_kinds.contains(&ConstrType::ConstrGenerated),
            not_null: definition.is_not_null
                || constraint_kinds
                    .iter()
                    .any(|kind| matches!(kind, ConstrType::ConstrNotnull | ConstrType::ConstrPrimary)),
            source,
            origin,
            definition: match source {
                ColumnSource::AlterColumnType => statement.rest_from(statement.after_name(origin.offset(), name)),
                ColumnSource::CreateTable | ColumnSource::AddColumn | ColumnSource::CompositeType => {
                    statement.item_from(statement.after_name(origin.offset(), name))
                }
            },
        })
    }

    /// The table or composite type the column belongs to.
    #[must_use]
    pub fn relation(&self) -> &str {
        &self.relation
    }

    /// The column's name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Its type.
    #[must_use]
    pub const fn data_type(&self) -> &DeclaredType {
        &self.data_type
    }

    /// The compression method it names, if any.
    #[must_use]
    pub fn compression(&self) -> Option<&str> {
        self.compression.as_deref()
    }

    /// The collation it names, if any.
    #[must_use]
    pub const fn collation(&self) -> Option<&QualifiedName> {
        self.collation.as_ref()
    }

    /// Whether the database generates its values as an identity.
    #[must_use]
    pub const fn is_identity(&self) -> bool {
        self.identity
    }

    /// Whether it is a stored generated column.
    #[must_use]
    pub const fn is_generated(&self) -> bool {
        self.generated
    }

    /// Whether it is declared `not null` or is a primary key column.
    #[must_use]
    pub const fn is_not_null(&self) -> bool {
        self.not_null
    }

    /// The statement that declares it.
    #[must_use]
    pub const fn source(&self) -> ColumnSource {
        self.source
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

/// The compression method `alter table ... alter column ... set compression`
/// gives a column.
#[derive(Debug, Clone)]
pub struct CompressionSetting {
    /// The table.
    relation: String,
    /// The column.
    column: String,
    /// The method named.
    method: String,
    /// Where the column's name is written.
    origin: Origin,
}

impl CompressionSetting {
    /// The method `method` set on `column` of `relation` at `origin`.
    #[must_use]
    pub fn new(relation: &str, column: &str, method: &str, origin: Origin) -> CompressionSetting {
        CompressionSetting {
            relation: relation.to_owned(),
            column: column.to_owned(),
            method: method.to_owned(),
            origin,
        }
    }

    /// The table.
    #[must_use]
    pub fn relation(&self) -> &str {
        &self.relation
    }

    /// The column.
    #[must_use]
    pub fn column(&self) -> &str {
        &self.column
    }

    /// The method named.
    #[must_use]
    pub fn method(&self) -> &str {
        &self.method
    }

    /// Where the column's name is written.
    #[must_use]
    pub const fn origin(&self) -> Origin {
        self.origin
    }
}
