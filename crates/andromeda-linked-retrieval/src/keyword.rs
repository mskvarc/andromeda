//! The three Entity members clause 4.21 lets a projection name beside
//! Attributes.

use andromeda_ngsi_ld_query::projection::term::ProjectionTerm;
use enumset::EnumSetType;

/// `id`, `type` or `scope`: members of an Entity's representation that a
/// clause 4.21 projection names by keyword rather than by expanded IRI.
#[derive(Debug, EnumSetType)]
pub enum Keyword {
    /// The entity identifier.
    Id,
    /// The entity type.
    Type,
    /// The entity scope.
    Scope,
}

impl Keyword {
    /// The projection term naming this keyword.
    #[must_use]
    pub const fn term(self) -> ProjectionTerm {
        match self {
            Keyword::Id => ProjectionTerm::Id,
            Keyword::Type => ProjectionTerm::Type,
            Keyword::Scope => ProjectionTerm::Scope,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::keyword::Keyword;
    use andromeda_ngsi_ld_query::projection::term::ProjectionTerm;
    use pretty_assertions::assert_eq;

    #[test]
    fn every_keyword_names_its_projection_term() {
        for (keyword, term) in [
            (Keyword::Id, ProjectionTerm::Id),
            (Keyword::Type, ProjectionTerm::Type),
            (Keyword::Scope, ProjectionTerm::Scope),
        ] {
            assert_eq!(keyword.term(), term);
        }
    }
}
