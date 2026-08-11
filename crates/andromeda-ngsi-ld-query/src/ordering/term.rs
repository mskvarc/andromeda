//! Ordering terms, before and after context expansion (clause 4.23).

use crate::ordering::collation::IcuCollation;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, reference::NgsiLdReference};
use serde::{Deserialize, Serialize};
use std::{fmt, str::FromStr};
use strum::{Display, EnumString};
use thiserror::Error;

/// A single parsed ordering term from the `orderBy` parameter.
///
/// `attr` is the raw request text, before it is classified and expanded into a
/// [`CompiledOrderTerm`]: at this point it is whatever the client wrote.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct OrderTerm {
    /// The attribute name or clause 4.23 keyword to sort by, as written.
    pub attr: NgsiLdReference,
    /// The sort direction.
    pub direction: OrderDirection,
}

/// What an `orderBy` term sorts on (clause 4.23).
///
/// `id` and `type` name structural members of the entity representation, not
/// attributes, so they have no IRI to expand to and they are read from their
/// own columns rather than from the payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OrderTarget {
    /// The entity identifier.
    Id,
    /// The entity types.
    Type,
    /// The entity scopes.
    Scope,
    /// One attribute's value, named by its expanded IRI (clause 5.2.4).
    Attribute(AttributeName),
}

/// A raw `orderBy` name, classified but not yet expanded.
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
    /// An attribute name, still compact.
    Attribute(&'a NgsiLdReference),
}

impl<'a> RawOrderTarget<'a> {
    /// The `id` keyword of clause 4.23.
    pub const ID_KEYWORD: &'static str = "id";
    /// The `type` keyword of clause 4.23.
    pub const TYPE_KEYWORD: &'static str = "type";
    /// The `scope` keyword of clause 4.23.
    pub const SCOPE_KEYWORD: &'static str = "scope";

    /// Classify a raw `orderBy` name.
    pub fn classify(raw: &'a NgsiLdReference) -> RawOrderTarget<'a> {
        match raw.as_str() {
            RawOrderTarget::ID_KEYWORD => RawOrderTarget::Id,
            RawOrderTarget::TYPE_KEYWORD => RawOrderTarget::Type,
            RawOrderTarget::SCOPE_KEYWORD => RawOrderTarget::Scope,
            // `&str` is unbounded, so a catch-all is the only way to close this
            // match; anything that is not one of the two keywords is an
            // attribute name.
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
    /// The term did not name a structural member or attribute.
    #[error("invalid orderBy member: {0}")]
    Member(String),
    /// The term used an unsupported direction.
    #[error("invalid orderBy direction: {0}")]
    Direction(String),
}

impl FromStr for OrderTerm {
    type Err = OrderTermError;

    fn from_str(raw: &str) -> Result<OrderTerm, OrderTermError> {
        let raw = raw.trim();
        let (member, direction) = match raw.split_once(';') {
            Some((member, direction)) => {
                let direction = direction
                    .trim()
                    .parse::<OrderDirection>()
                    .map_err(|_| OrderTermError::Direction(direction.trim().to_string()))?;
                (member.trim(), direction)
            }
            None => match raw.strip_prefix('!') {
                Some(member) => (member, OrderDirection::Desc),
                None => (raw, OrderDirection::Asc),
            },
        };
        let attr = member.parse::<NgsiLdReference>().map_err(|_| OrderTermError::Member(member.to_string()))?;
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
        match self.direction {
            OrderDirection::Asc => formatter.write_str(self.attr.as_str()),
            OrderDirection::Desc | OrderDirection::DistAsc | OrderDirection::DistDesc => {
                write!(formatter, "{};{}", self.attr, self.direction)
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
    pub const fn is_distance(&self) -> bool {
        matches!(self, OrderDirection::DistAsc | OrderDirection::DistDesc)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_order_by_term_of_id_or_type_is_not_expanded() {
        // Clause 4.23: `id` and `type` name members of the representation, not
        // attributes, so no context lookup applies to them and neither reaches
        // the attribute arm that performs one.
        let id = "id".parse().expect("reference");
        let type_ = "type".parse().expect("reference");
        let scope = "scope".parse().expect("reference");
        assert_eq!(RawOrderTarget::classify(&id), RawOrderTarget::Id);
        assert_eq!(RawOrderTarget::classify(&type_), RawOrderTarget::Type);
        assert_eq!(RawOrderTarget::classify(&scope), RawOrderTarget::Scope);
    }

    #[test]
    fn any_other_order_by_term_is_classified_as_an_attribute() {
        let speed = "speed".parse().expect("reference");
        assert_eq!(RawOrderTarget::classify(&speed), RawOrderTarget::Attribute(&speed));
    }
}
