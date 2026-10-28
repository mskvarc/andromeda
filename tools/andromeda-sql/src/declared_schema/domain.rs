//! A domain, as a migration declares it.

use crate::declared_schema::{declared_type::DeclaredType, origin::Origin, qualified_name::QualifiedName, statement_context::StatementContext};
use pg_query::protobuf::CreateDomainStmt;

/// One `create domain`.
#[derive(Debug, Clone)]
pub struct DeclaredDomain {
    /// The domain's name.
    name: String,
    /// The type it constrains.
    base: DeclaredType,
    /// The collation it names, if any.
    collation: Option<QualifiedName>,
    /// Where its name is written.
    origin: Origin,
}

impl DeclaredDomain {
    /// The domain `domain` declares, read from `statement`; `None` for one
    /// without a name or a type.
    #[must_use]
    pub fn read(domain: &CreateDomainStmt, statement: &StatementContext<'_>) -> Option<DeclaredDomain> {
        let name = QualifiedName::of_nodes(&domain.domainname)?.name().to_owned();
        Some(DeclaredDomain {
            base: DeclaredType::of(domain.type_name.as_ref()?)?,
            collation: domain.coll_clause.as_ref().and_then(|clause| QualifiedName::of_nodes(&clause.collname)),
            origin: statement.origin_of_name(&name),
            name,
        })
    }

    /// The domain's name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The type it constrains.
    #[must_use]
    pub const fn base(&self) -> &DeclaredType {
        &self.base
    }

    /// The collation it names, if any.
    #[must_use]
    pub const fn collation(&self) -> Option<&QualifiedName> {
        self.collation.as_ref()
    }

    /// Where its name is written.
    #[must_use]
    pub const fn origin(&self) -> Origin {
        self.origin
    }
}
