//! An `orderBy` term (clause 4.23) as the client wrote it: a value path or keyword, and a direction.

use crate::{
    filter::{target_language::LanguageSelector, trailing_group::TrailingGroup, value_path::ValuePath},
    ordering::{ordering_direction::OrderingDirection, ordering_error::OrderingError},
};
use serde::{Deserialize, Serialize};
use std::{fmt, str::FromStr};

/// A parsed `orderBy` term. `path` is the raw request text, before it is classified and expanded into a [`CompiledOrderingTerm`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct WrittenOrderingTerm {
    /// The attribute path or clause 4.23 keyword to sort by, as written. Clause 4.23.3 delegates `AttrName` to clause 4.9, and its examples (`address[city]`, `name.observedAt`) are both the clause 4.9 `ValuePath` production, which is why this is a whole path rather than one name.
    pub path: ValuePath,
    /// The sort direction.
    pub direction: OrderingDirection,
}

impl FromStr for WrittenOrderingTerm {
    type Err = OrderingError;

    fn from_str(raw: &str) -> Result<WrittenOrderingTerm, OrderingError> {
        let raw = raw.trim();
        let (member, direction) = match raw.split_once(';') {
            Some((member, direction)) => {
                let direction = direction.trim().parse::<OrderingDirection>().map_err(OrderingError::InvalidDirection)?;
                (member.trim(), direction)
            }
            None => (raw, OrderingDirection::Asc),
        };
        let path = member.parse::<ValuePath>().map_err(OrderingError::Path)?;
        if path.trailing == Some(TrailingGroup::AllLanguages) {
            return Err(OrderingError::LanguageSuppression);
        }
        if path.target_language().is_some_and(LanguageSelector::is_range) {
            return Err(OrderingError::LanguageRange);
        }
        Ok(WrittenOrderingTerm { path, direction })
    }
}

impl TryFrom<String> for WrittenOrderingTerm {
    type Error = OrderingError;

    fn try_from(value: String) -> Result<WrittenOrderingTerm, OrderingError> {
        value.parse()
    }
}

// A derive cannot conditionally omit the default direction while appending
// non-default directions with the clause-4.23 separator.
impl fmt::Display for WrittenOrderingTerm {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let member = self.path.to_string();
        match self.direction {
            OrderingDirection::Asc => formatter.write_str(&member),
            OrderingDirection::Desc | OrderingDirection::DistAsc | OrderingDirection::DistDesc => {
                write!(formatter, "{member};{}", self.direction)
            }
        }
    }
}

impl From<WrittenOrderingTerm> for String {
    fn from(value: WrittenOrderingTerm) -> String {
        value.to_string()
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        filter::target_language::LanguageSelector,
        ordering::{ordering_direction::OrderingDirection, ordering_error::OrderingError, written_ordering_term::WrittenOrderingTerm},
    };
    use pretty_assertions::assert_eq;

    /// The path of a term that parses.
    fn term(text: &str) -> WrittenOrderingTerm {
        text.parse().expect("a clause 4.23.3 ordering term")
    }

    /// Clause 4.23.3 EXAMPLE 5: `?orderBy=name.observedAt`.
    #[test]
    fn a_dotted_attribute_path_is_accepted_as_an_ordering_member() {
        let parsed = term("name.observedAt");
        assert_eq!(parsed.path.attribute.as_str(), "name");
        assert_eq!(parsed.path.sub_attributes.len(), 1);
        assert_eq!(parsed.path.sub_attributes[0].as_str(), "observedAt");
        assert_eq!(parsed.direction, OrderingDirection::Asc);
    }

    /// Clause 4.23.3 EXAMPLE 4: `?orderBy=address[city]`.
    #[test]
    fn a_compound_member_path_is_accepted_as_an_ordering_member() {
        let parsed = term("address[city]");
        assert_eq!(parsed.path.attribute.as_str(), "address");
        let members = parsed.path.members().expect("a member path");
        assert_eq!(members.addressed.len(), 1);
        assert_eq!(members.addressed[0].as_str(), "city");
    }

    /// A direction still binds to the whole path, not to its last segment.
    #[test]
    fn a_direction_applies_to_the_whole_path() {
        let parsed = term("sensor.rawdata[airquality.particulate];desc");
        assert_eq!(parsed.direction, OrderingDirection::Desc);
        assert_eq!(parsed.path.attribute.as_str(), "sensor");
        assert_eq!(parsed.path.sub_attributes.len(), 1);
        assert_eq!(parsed.path.members().map(|members| members.addressed.len()), Some(2));
    }

    /// Clause 4.23.3's grammar is `AttrName`-based, so the clause 4.9 linked-entity axis is not an ordering member.
    #[test]
    fn a_linked_entity_traversal_is_not_an_ordering_member() {
        for rejected in ["rel{attr}", "rel{Building,Store:attr}"] {
            let error = rejected.parse::<WrittenOrderingTerm>().expect_err("a hop is not an ordering member");
            assert!(matches!(error, OrderingError::Path(_)), "{rejected}: got {error:?}");
        }
    }

    /// The `[*]` marker belongs to the clause 4.9 `q` grammar alone.
    #[test]
    fn the_language_suppression_marker_is_not_an_ordering_member() {
        let error = "name[*]".parse::<WrittenOrderingTerm>().expect_err("[*] orders by no defined value");
        assert!(matches!(error, OrderingError::LanguageSuppression), "got {error:?}");
    }

    /// Clause 4.23.3 orders by one value; a range selects several languages.
    #[test]
    fn a_language_range_is_not_an_ordering_member_but_a_full_tag_is() {
        let error = "name[de-*-DE]".parse::<WrittenOrderingTerm>().expect_err("a range orders by no one value");
        assert!(matches!(error, OrderingError::LanguageRange), "got {error:?}");
        let parsed = term("name[en-GB]");
        assert_eq!(parsed.path.target_language().map(LanguageSelector::as_str), Some("en-GB"));
        assert_eq!(parsed.to_string(), "name[en-GB]");
    }

    /// A term is carried in the request text, so it has to spell itself back.
    #[test]
    fn every_shape_of_ordering_term_round_trips_through_its_text() {
        for text in [
            "name",
            "name.observedAt",
            "address[city]",
            "sensor.rawdata[airquality.particulate]",
            "ex:address[city]",
            "name;desc",
            "name.observedAt;desc",
            "location;dist-asc",
            "id",
        ] {
            let parsed = term(text);
            assert_eq!(parsed.to_string(), text, "{text} must round-trip");
            assert_eq!(parsed.to_string().parse::<WrittenOrderingTerm>().expect("re-parses"), parsed, "{text}");
        }
    }

    /// Clause 4.23.3 spells a descending term `AttrName;desc`; a leading `!` is not part of the grammar.
    #[test]
    fn a_bang_prefix_is_not_an_ordering_member() {
        for rejected in ["!name", "!name.observedAt", "!name;asc"] {
            let error = rejected.parse::<WrittenOrderingTerm>().expect_err("a bang prefix is not an ordering member");
            assert!(
                matches!(error, OrderingError::Path(_) | OrderingError::InvalidDirection(_)),
                "{rejected}: got {error:?}"
            );
        }
    }

    #[test]
    fn a_malformed_member_is_rejected_rather_than_read_as_a_name() {
        for rejected in ["a.", "a[", "a[]", "a[b][c]", "a[b].c", ""] {
            assert!(rejected.parse::<WrittenOrderingTerm>().is_err(), "{rejected}");
        }
    }

    #[test]
    fn an_unsupported_direction_is_rejected() {
        let error = "name;sideways".parse::<WrittenOrderingTerm>().expect_err("no such direction");
        assert!(matches!(error, OrderingError::InvalidDirection(_)), "got {error:?}");
    }
}
