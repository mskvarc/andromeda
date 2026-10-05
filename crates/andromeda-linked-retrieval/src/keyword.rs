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

    /// The keyword a member key spells, if it spells one.
    #[must_use]
    pub fn of_key(key: &str) -> Option<Keyword> {
        match key {
            ProjectionTerm::ID_KEYWORD => Some(Keyword::Id),
            ProjectionTerm::TYPE_KEYWORD => Some(Keyword::Type),
            ProjectionTerm::SCOPE_KEYWORD => Some(Keyword::Scope),
            // `&str` is unbounded; every other key names no keyword.
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::keyword::Keyword;
    use andromeda_ngsi_ld_query::projection::term::ProjectionTerm;
    use pretty_assertions::assert_eq;

    #[test]
    fn every_keyword_round_trips_through_its_key_and_term() {
        for (key, keyword, term) in [
            ("id", Keyword::Id, ProjectionTerm::Id),
            ("type", Keyword::Type, ProjectionTerm::Type),
            ("scope", Keyword::Scope, ProjectionTerm::Scope),
        ] {
            assert_eq!(Keyword::of_key(key), Some(keyword));
            assert_eq!(keyword.term(), term);
        }
        assert_eq!(Keyword::of_key("name"), None);
    }
}
