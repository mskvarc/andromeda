//! A named object a migration creates.

use crate::declared_schema::origin::Origin;

/// What kind of object a name belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::Display)]
#[strum(serialize_all = "kebab-case")]
pub enum ObjectKind {
    /// A table.
    Table,
    /// A view.
    View,
    /// A composite type.
    CompositeType,
    /// A domain.
    Domain,
    /// A column of a table or a field of a composite type.
    Column,
    /// A constraint.
    Constraint,
    /// An index.
    Index,
    /// An extended statistics object.
    Statistics,
    /// A function or a procedure.
    Function,
    /// A trigger.
    Trigger,
    /// An extension.
    Extension,
}

/// One object a migration creates, by name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreatedObject {
    /// What kind of object it is.
    kind: ObjectKind,
    /// Its name.
    name: String,
    /// Where its name is written.
    origin: Origin,
}

impl CreatedObject {
    /// The object of `kind` named `name` at `origin`.
    #[must_use]
    pub fn new(kind: ObjectKind, name: impl Into<String>, origin: Origin) -> CreatedObject {
        CreatedObject {
            kind,
            name: name.into(),
            origin,
        }
    }

    /// What kind of object it is.
    #[must_use]
    pub const fn kind(&self) -> ObjectKind {
        self.kind
    }

    /// Its name.
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
