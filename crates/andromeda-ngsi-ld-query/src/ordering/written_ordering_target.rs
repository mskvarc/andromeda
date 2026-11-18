//! What an `orderBy` term sorts on, classified but not yet expanded (clause 4.23).

use crate::filter::value_path::ValuePath;
use andromeda_ngsi_ld_vocabulary::{compact::properties as compact_properties, keyword};

/// A raw `orderBy` path, classified but not yet expanded. Classification is separate from expansion so the clause 4.23 keyword set is stated once, while the caller decides how the expansion (a JSON-LD lookup, needed only for the attribute arm) happens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WrittenOrderingTarget<'a> {
    /// The `id` keyword.
    Id,
    /// The `type` keyword.
    Type,
    /// The `scope` keyword.
    Scope,
    /// The `createdAt` keyword.
    CreatedAt,
    /// The `modifiedAt` keyword.
    ModifiedAt,
    /// An attribute path, still compact.
    Attribute(&'a ValuePath),
}

impl<'a> WrittenOrderingTarget<'a> {
    /// Classifies a raw `orderBy` path. A keyword is only a keyword when the path names it and nothing else: clause 4.23.3 spells each as a bare `AttrName`, so `id.foo` descends into an attribute spelled `id`, and `name.createdAt` is an attribute path into a temporal sub-member rather than the entity's own `createdAt` column.
    #[must_use]
    pub fn classify(raw: &'a ValuePath) -> WrittenOrderingTarget<'a> {
        if !raw.is_simple() {
            return WrittenOrderingTarget::Attribute(raw);
        }
        match raw.attribute.as_str() {
            keyword::ID_ALIAS => WrittenOrderingTarget::Id,
            keyword::TYPE_ALIAS => WrittenOrderingTarget::Type,
            compact_properties::SCOPE => WrittenOrderingTarget::Scope,
            compact_properties::CREATED_AT => WrittenOrderingTarget::CreatedAt,
            compact_properties::MODIFIED_AT => WrittenOrderingTarget::ModifiedAt,
            // `&str` is unbounded; any name that is not a keyword is an attribute name.
            _other => WrittenOrderingTarget::Attribute(raw),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        filter::value_path::ValuePath,
        ordering::{written_ordering_target::WrittenOrderingTarget, written_ordering_term::WrittenOrderingTerm},
    };
    use pretty_assertions::assert_eq;

    /// The path of a term that parses.
    fn path(text: &str) -> ValuePath {
        text.parse::<WrittenOrderingTerm>().expect("a clause 4.23.3 ordering term").path
    }

    #[test]
    fn an_order_by_term_of_id_type_or_scope_is_not_expanded() {
        // Clause 4.23: `id`, `type` and `scope` name members of the
        // representation, not attributes, so no context lookup applies to them
        // and none reaches the attribute arm that performs one.
        for (text, expected) in [
            ("id", WrittenOrderingTarget::Id),
            ("type", WrittenOrderingTarget::Type),
            ("scope", WrittenOrderingTarget::Scope),
        ] {
            assert_eq!(WrittenOrderingTarget::classify(&path(text)), expected, "{text}");
        }
    }

    /// Clause 4.8: `createdAt` and `modifiedAt` are sortable system timestamps read from their own columns, like `id`, `type` and `scope`.
    #[test]
    fn the_system_timestamp_keywords_classify_as_their_members() {
        for (text, expected) in [
            ("createdAt", WrittenOrderingTarget::CreatedAt),
            ("modifiedAt", WrittenOrderingTarget::ModifiedAt),
        ] {
            assert_eq!(WrittenOrderingTarget::classify(&path(text)), expected, "{text}");
        }
    }

    /// A path headed by `createdAt`/`modifiedAt` descends into an attribute spelled that way, exactly as `id.foo` does.
    #[test]
    fn a_system_timestamp_keyword_carrying_a_path_is_an_attribute() {
        for text in ["createdAt.foo", "modifiedAt[a]", "name.createdAt", "name.modifiedAt"] {
            assert!(
                matches!(WrittenOrderingTarget::classify(&path(text)), WrittenOrderingTarget::Attribute(_)),
                "{text} descends past the keyword"
            );
        }
    }

    #[test]
    fn any_other_order_by_term_is_classified_as_an_attribute() {
        let speed = path("speed");
        assert_eq!(WrittenOrderingTarget::classify(&speed), WrittenOrderingTarget::Attribute(&speed));
    }

    /// A keyword only names a structural member when the path stops there; the entity identifier has no sub-attribute.
    #[test]
    fn a_keyword_carrying_a_path_is_an_attribute_rather_than_the_member() {
        for text in ["id.foo", "type[a]", "scope.a"] {
            assert!(
                matches!(WrittenOrderingTarget::classify(&path(text)), WrittenOrderingTarget::Attribute(_)),
                "{text} descends past the keyword"
            );
        }
    }
}
