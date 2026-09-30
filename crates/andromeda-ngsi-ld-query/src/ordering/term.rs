//! Ordering terms, before and after context expansion (clause 4.23).

use crate::{
    error::QueryError,
    filter::{path::AttrPath, serialize::serialize_attr_path, value_path::parse_attribute_path},
    ordering::collation::IcuCollation,
};
use serde::{Deserialize, Serialize};
use std::{fmt, str::FromStr};
use strum::{Display, EnumString};
use thiserror::Error;

/// A single parsed ordering term from the `orderBy` parameter.
///
/// `attr` is the raw request text, before it is classified and expanded into a
/// [`CompiledOrderTerm`]: at this point its names are whatever the client
/// wrote.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct OrderTerm {
    /// The attribute path or clause 4.23 keyword to sort by, as written.
    ///
    /// Clause 4.23.3 gives `OrderingTerm = AttrName *1(DirectionTerm)
    /// *(thenOp OrderingTerm)` and says "See clause 4.9 for the definition of
    /// `AttrName`". Its own examples settle what that reaches: EXAMPLE 4
    /// `?orderBy=address[city]` navigates "a sub-item within a Property Value
    /// defined as a complex JSON object", and EXAMPLE 5 `?orderBy=name.observedAt`
    /// is "an attribute path that is defined by the production rule Attribute,
    /// as a dot-separated list of names". Both are the clause 4.9 `ValuePath`
    /// production, which is why this is a whole path rather than one name.
    pub attr: AttrPath,
    /// The sort direction.
    pub direction: OrderDirection,
}

/// What an `orderBy` term sorts on (clause 4.23).
///
/// `id`, `type`, `scope`, `createdAt` and `modifiedAt` name structural members
/// of the entity representation, not attributes, so they have no IRI to expand
/// to and they are read from their own columns rather than from the payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OrderTarget {
    /// The entity identifier.
    Id,
    /// The entity types.
    Type,
    /// The entity scopes.
    Scope,
    /// The instant the entity was created (clause 4.8 `createdAt`).
    CreatedAt,
    /// The instant the entity's payload was last modified (clause 4.8
    /// `modifiedAt`).
    ModifiedAt,
    /// One attribute's value, addressed by a path whose every name is an
    /// expanded IRI (clause 5.2.4).
    Attribute(AttrPath),
}

/// A raw `orderBy` path, classified but not yet expanded.
///
/// Classification is separated from expansion so the clause 4.23 keyword set is
/// stated once, while the caller — which needs a JSON-LD lookup, and therefore
/// an `await`, only for the attribute arm — decides how the expansion happens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RawOrderTarget<'a> {
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
    Attribute(&'a AttrPath),
}

impl<'a> RawOrderTarget<'a> {
    /// The `id` keyword of clause 4.23.
    pub const ID_KEYWORD: &'static str = "id";
    /// The `type` keyword of clause 4.23.
    pub const TYPE_KEYWORD: &'static str = "type";
    /// The `scope` keyword of clause 4.23.
    pub const SCOPE_KEYWORD: &'static str = "scope";
    /// The `createdAt` system timestamp keyword (clause 4.8).
    pub const CREATED_AT_KEYWORD: &'static str = "createdAt";
    /// The `modifiedAt` system timestamp keyword (clause 4.8).
    pub const MODIFIED_AT_KEYWORD: &'static str = "modifiedAt";

    /// Classify a raw `orderBy` path.
    ///
    /// A keyword is only a keyword when the path names it and nothing else.
    /// Clause 4.23.3 spells each as a bare `AttrName`, so `id.foo` descends
    /// into an attribute that happens to be spelled `id` rather than naming the
    /// entity identifier — which carries no sub-attribute to descend into. The
    /// same reading keeps `name.createdAt` an attribute path into a temporal
    /// sub-member rather than the entity's own `createdAt` column.
    #[must_use]
    pub fn classify(raw: &'a AttrPath) -> RawOrderTarget<'a> {
        if !raw.is_simple() {
            return RawOrderTarget::Attribute(raw);
        }
        match raw.attr.as_str() {
            RawOrderTarget::ID_KEYWORD => RawOrderTarget::Id,
            RawOrderTarget::TYPE_KEYWORD => RawOrderTarget::Type,
            RawOrderTarget::SCOPE_KEYWORD => RawOrderTarget::Scope,
            RawOrderTarget::CREATED_AT_KEYWORD => RawOrderTarget::CreatedAt,
            RawOrderTarget::MODIFIED_AT_KEYWORD => RawOrderTarget::ModifiedAt,
            // `&str` is unbounded, so a catch-all is the only way to close this
            // match; anything that is not one of the keywords is an attribute
            // name.
            _other => RawOrderTarget::Attribute(raw),
        }
    }
}

/// A single term in an `orderBy` clause after context expansion.
#[derive(Debug, Clone)]
pub struct CompiledOrderTerm {
    /// What the term sorts on.
    pub target: OrderTarget,
    /// The sort direction (ascending, descending, or distance-based).
    pub direction: OrderDirection,
    /// ICU collation identifier for string sorting (S4.23).
    pub collation: Option<IcuCollation>,
}

/// Sort direction for `orderBy` terms per NGSI-LD S4.23.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Display, EnumString)]
#[strum(serialize_all = "kebab-case")]
pub enum OrderDirection {
    /// Sort in ascending order.
    Asc,
    /// Sort in descending order.
    Desc,
    /// Sort by distance in ascending order from the geo query reference geometry (S4.23).
    DistAsc,
    /// Sort by distance in descending order from the geo query reference geometry (S4.23).
    DistDesc,
}

/// A malformed clause-4.23 ordering term.
#[derive(Debug, Error)]
pub enum OrderTermError {
    /// The term did not satisfy the clause 4.9 `ValuePath` production that
    /// clause 4.23.3 delegates its `AttrName` to.
    #[error("invalid orderBy member: {0}")]
    Member(#[source] QueryError),
    /// The term used an unsupported direction.
    #[error("invalid orderBy direction: {0}")]
    Direction(#[source] strum::ParseError),
    /// The term carried the `[*]` language-suppression marker.
    ///
    /// Clause 4.9 gives `[*]` to a `q` predicate, where it matches a
    /// `LanguageProperty` against every language rather than the one `lang`
    /// selects. Clause 4.23.3 has no such marker: its grammar is `AttrName`
    /// based and its prose names only the dotted and bracketed paths, so an
    /// ordering that suppressed language selection would be ordering by a value
    /// the clause never says how to choose.
    #[error("invalid orderBy member: the [*] language marker belongs to the q filter grammar, not to orderBy")]
    LanguageSuppression,
}

impl FromStr for OrderTerm {
    type Err = OrderTermError;

    fn from_str(raw: &str) -> Result<OrderTerm, OrderTermError> {
        let raw = raw.trim();
        let (member, direction) = match raw.split_once(';') {
            Some((member, direction)) => {
                let direction = direction.trim().parse::<OrderDirection>().map_err(OrderTermError::Direction)?;
                (member.trim(), direction)
            }
            None => match raw.strip_prefix('!') {
                Some(member) => (member, OrderDirection::Desc),
                None => (raw, OrderDirection::Asc),
            },
        };
        let attr = parse_attribute_path(member).map_err(OrderTermError::Member)?;
        if attr.match_all_languages {
            return Err(OrderTermError::LanguageSuppression);
        }
        Ok(OrderTerm { attr, direction })
    }
}

impl TryFrom<String> for OrderTerm {
    type Error = OrderTermError;

    fn try_from(value: String) -> Result<OrderTerm, OrderTermError> {
        value.parse()
    }
}

// A derive cannot conditionally omit the default direction while appending
// non-default directions with the clause-4.23 separator.
impl fmt::Display for OrderTerm {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let member = serialize_attr_path(&self.attr);
        match self.direction {
            OrderDirection::Asc => formatter.write_str(&member),
            OrderDirection::Desc | OrderDirection::DistAsc | OrderDirection::DistDesc => {
                write!(formatter, "{member};{}", self.direction)
            }
        }
    }
}

impl From<OrderTerm> for String {
    fn from(value: OrderTerm) -> String {
        value.to_string()
    }
}

impl OrderDirection {
    /// Whether this is a distance-based ordering direction.
    #[must_use]
    pub const fn is_distance(&self) -> bool {
        matches!(self, OrderDirection::DistAsc | OrderDirection::DistDesc)
    }
}

#[cfg(test)]
mod tests {
    use crate::ordering::term::{OrderDirection, OrderTerm, OrderTermError, RawOrderTarget};
    use pretty_assertions::assert_eq;

    /// The path of a term that parses.
    fn term(text: &str) -> OrderTerm {
        text.parse().expect("a clause 4.23.3 ordering term")
    }

    #[test]
    fn an_order_by_term_of_id_type_or_scope_is_not_expanded() {
        // Clause 4.23: `id`, `type` and `scope` name members of the
        // representation, not attributes, so no context lookup applies to them
        // and none reaches the attribute arm that performs one.
        for (text, expected) in [("id", RawOrderTarget::Id), ("type", RawOrderTarget::Type), ("scope", RawOrderTarget::Scope)] {
            let parsed = term(text);
            assert_eq!(RawOrderTarget::classify(&parsed.attr), expected, "{text}");
        }
    }

    /// Clause 4.8: `createdAt` and `modifiedAt` are sortable system timestamps
    /// of the entity, read from their own columns like `id`, `type` and
    /// `scope`, so a bare keyword classifies as the member rather than as an
    /// attribute path.
    #[test]
    fn the_system_timestamp_keywords_classify_as_their_members() {
        for (text, expected) in [("createdAt", RawOrderTarget::CreatedAt), ("modifiedAt", RawOrderTarget::ModifiedAt)] {
            let parsed = term(text);
            assert_eq!(RawOrderTarget::classify(&parsed.attr), expected, "{text}");
        }
    }

    /// A dotted path whose head is `createdAt`/`modifiedAt` descends into an
    /// attribute spelled that way, not the entity's own timestamp column, so it
    /// stays an attribute path exactly as `id.foo` does.
    #[test]
    fn a_system_timestamp_keyword_carrying_a_path_is_an_attribute() {
        for text in ["createdAt.foo", "modifiedAt[a]", "name.createdAt", "name.modifiedAt"] {
            let parsed = term(text);
            assert!(
                matches!(RawOrderTarget::classify(&parsed.attr), RawOrderTarget::Attribute(_)),
                "{text} descends past the keyword"
            );
        }
    }

    #[test]
    fn any_other_order_by_term_is_classified_as_an_attribute() {
        let speed = term("speed");
        assert_eq!(RawOrderTarget::classify(&speed.attr), RawOrderTarget::Attribute(&speed.attr));
    }

    /// A keyword only names a structural member when the path stops there. The
    /// entity identifier has no sub-attribute, so `id.foo` is an attribute
    /// path.
    #[test]
    fn a_keyword_carrying_a_path_is_an_attribute_rather_than_the_member() {
        for text in ["id.foo", "type[a]", "scope.a"] {
            let parsed = term(text);
            assert!(
                matches!(RawOrderTarget::classify(&parsed.attr), RawOrderTarget::Attribute(_)),
                "{text} descends past the keyword"
            );
        }
    }

    /// Clause 4.23.3 EXAMPLE 5: `?orderBy=name.observedAt`.
    #[test]
    fn a_dotted_attribute_path_is_accepted_as_an_ordering_member() {
        let parsed = term("name.observedAt");
        assert_eq!(parsed.attr.attr.as_str(), "name");
        assert_eq!(parsed.attr.sub_attrs.len(), 1);
        assert_eq!(parsed.attr.sub_attrs[0].as_str(), "observedAt");
        assert_eq!(parsed.direction, OrderDirection::Asc);
    }

    /// Clause 4.23.3 EXAMPLE 4: `?orderBy=address[city]`.
    #[test]
    fn a_compound_member_path_is_accepted_as_an_ordering_member() {
        let parsed = term("address[city]");
        assert_eq!(parsed.attr.attr.as_str(), "address");
        assert_eq!(parsed.attr.value_path.len(), 1);
        assert_eq!(parsed.attr.value_path[0].as_str(), "city");
    }

    /// A direction still binds to the whole path, not to its last segment.
    #[test]
    fn a_direction_applies_to_the_whole_path() {
        let parsed = term("sensor.rawdata[airquality.particulate];desc");
        assert_eq!(parsed.direction, OrderDirection::Desc);
        assert_eq!(parsed.attr.attr.as_str(), "sensor");
        assert_eq!(parsed.attr.sub_attrs.len(), 1);
        assert_eq!(parsed.attr.value_path.len(), 2);
    }

    /// Clause 4.23.3's grammar is `AttrName`-based and its prose names only the
    /// dotted and bracketed paths, so the clause 4.9 linked-entity axis is not
    /// an ordering member.
    #[test]
    fn a_linked_entity_traversal_is_not_an_ordering_member() {
        for rejected in ["rel{attr}", "rel{Building,Store:attr}"] {
            let error = rejected.parse::<OrderTerm>().expect_err("a hop is not an ordering member");
            assert!(matches!(error, OrderTermError::Member(_)), "{rejected}: got {error:?}");
        }
    }

    /// The `[*]` marker belongs to the clause 4.9 `q` grammar alone.
    #[test]
    fn the_language_suppression_marker_is_not_an_ordering_member() {
        let error = "name[*]".parse::<OrderTerm>().expect_err("[*] orders by no defined value");
        assert!(matches!(error, OrderTermError::LanguageSuppression), "got {error:?}");
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
            assert_eq!(parsed.to_string().parse::<OrderTerm>().expect("re-parses"), parsed, "{text}");
        }
    }

    /// The `!` prefix is the shorthand for a descending term, and it renders
    /// back in the `;desc` spelling the grammar defines.
    #[test]
    fn the_bang_prefix_is_read_as_a_descending_term() {
        let parsed = term("!name.observedAt");
        assert_eq!(parsed.direction, OrderDirection::Desc);
        assert_eq!(parsed.to_string(), "name.observedAt;desc");
    }

    #[test]
    fn a_malformed_member_is_rejected_rather_than_read_as_a_name() {
        for rejected in ["a.", "a[", "a[]", "a[b][c]", "a[b].c", ""] {
            assert!(rejected.parse::<OrderTerm>().is_err(), "{rejected}");
        }
    }

    #[test]
    fn an_unsupported_direction_is_rejected() {
        let error = "name;sideways".parse::<OrderTerm>().expect_err("no such direction");
        assert!(matches!(error, OrderTermError::Direction(_)), "got {error:?}");
    }
}
