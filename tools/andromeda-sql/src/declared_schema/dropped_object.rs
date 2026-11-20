//! A table, a function or a schema a migration drops, so the schema the
//! migrations leave behind can be told from every object they ever created.

use crate::declared_schema::{created_object::ObjectKind, origin::Origin, qualified_name::QualifiedName, statement_context::StatementContext};
use pg_query::{
    NodeEnum,
    protobuf::{DropStmt, ObjectType},
};

/// One table, function or schema a `drop` statement removes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DroppedObject {
    /// [`ObjectKind::Table`], [`ObjectKind::Function`] or [`ObjectKind::Schema`].
    kind: ObjectKind,
    /// The object, a schema with no qualifier.
    name: QualifiedName,
    /// Where the `drop` statement starts.
    origin: Origin,
}

impl DroppedObject {
    /// Every table, function and schema `drop` removes, read from
    /// `statement`; a drop of any other kind of object yields none.
    #[must_use]
    pub fn read_all(drop: &DropStmt, statement: &StatementContext<'_>) -> Vec<DroppedObject> {
        let kind = match ObjectType::try_from(drop.remove_type) {
            Ok(ObjectType::ObjectTable) => ObjectKind::Table,
            Ok(ObjectType::ObjectFunction) => ObjectKind::Function,
            Ok(ObjectType::ObjectSchema) => ObjectKind::Schema,
            Ok(_) | Err(_) => return Vec::new(),
        };
        let origin = statement.migration().origin(statement.span().start);
        drop.objects
            .iter()
            .filter_map(|object| match &object.node {
                Some(NodeEnum::List(table)) => QualifiedName::of_nodes(&table.items),
                Some(NodeEnum::ObjectWithArgs(function)) => QualifiedName::of_nodes(&function.objname),
                Some(NodeEnum::String(schema)) => Some(QualifiedName::unqualified(schema.sval.as_str())),
                Some(_) | None => None,
            })
            .map(|name| DroppedObject { kind, name, origin })
            .collect()
    }

    /// [`ObjectKind::Table`], [`ObjectKind::Function`] or [`ObjectKind::Schema`].
    #[must_use]
    pub const fn kind(&self) -> ObjectKind {
        self.kind
    }

    /// The object, a schema with no qualifier.
    #[must_use]
    pub const fn name(&self) -> &QualifiedName {
        &self.name
    }

    /// Where the `drop` statement starts.
    #[must_use]
    pub const fn origin(&self) -> Origin {
        self.origin
    }
}
