//! Reading a comma-separated list parameter.
//!
//! Entity identifiers, attribute names, dataset identifiers, aggregation
//! methods, ordering terms and `options` tokens travel as one comma-separated
//! string in a query string or a query body member. Every reader splits such a
//! list here: each member is trimmed of the whitespace around it, and an empty
//! member is malformed input whatever the list enumerates, so the splitting
//! rule and its refusal are one concept shared by every reader.

use std::{error::Error as StdError, str::FromStr};
use strum::Display;
use thiserror::Error;

/// What a comma-separated list enumerates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Display)]
pub enum ListedItem {
    /// Entity identifiers (clause 4.6.2).
    #[strum(serialize = "entity identifier")]
    EntityIdentifier,
    /// Attribute names (clause 4.6.2).
    #[strum(serialize = "attribute name")]
    AttributeName,
    /// Dataset identifiers (clause 4.5.5).
    #[strum(serialize = "dataset identifier")]
    DatasetIdentifier,
    /// Aggregation methods (clause 4.5.19).
    #[strum(serialize = "aggregation method")]
    AggregationMethod,
    /// Ordering terms (clause 4.23).
    #[strum(serialize = "ordering term")]
    OrderingTerm,
    /// `options` tokens (clause 5.5.4 and Table 6.3.1-1).
    #[strum(serialize = "options token")]
    OptionsToken,
}

/// A comma-separated list that cannot be read as written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum CommaSeparatedListError {
    /// Two commas, or a leading or trailing comma, left a member with no text.
    #[error("{list} list contains an empty member")]
    EmptyMember {
        /// What the list enumerates.
        list: ListedItem,
    },
}

/// A comma-separated list member that cannot be read: an empty one, or one
/// its element type refuses.
#[derive(Debug, Error)]
pub enum ListedMemberError<E: StdError + 'static> {
    /// A member is empty.
    #[error(transparent)]
    List(#[from] CommaSeparatedListError),
    /// A member is no element of the list.
    #[error(transparent)]
    Member(E),
}

/// The members of the comma-separated list `raw` enumerating `list`, each
/// trimmed of the whitespace around it.
///
/// # Errors
///
/// Yields [`CommaSeparatedListError::EmptyMember`] in place of every empty
/// member.
pub fn listed_members(raw: &str, list: ListedItem) -> impl Iterator<Item = Result<&str, CommaSeparatedListError>> {
    raw.split(',').map(str::trim).map(move |member| match member {
        "" => Err(CommaSeparatedListError::EmptyMember { list }),
        member => Ok(member),
    })
}

/// The members of the comma-separated list `raw` enumerating `list`, each
/// read as a `T`.
///
/// # Errors
///
/// Returns [`ListedMemberError::List`] for the first empty member and
/// [`ListedMemberError::Member`] for the first member `T` refuses.
pub fn parse_listed<T>(raw: &str, list: ListedItem) -> Result<Vec<T>, ListedMemberError<T::Err>>
where
    T: FromStr,
    T::Err: StdError + 'static,
{
    listed_members(raw, list)
        .map(|member| member?.parse().map_err(ListedMemberError::Member))
        .collect()
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::comma_separated_list::{CommaSeparatedListError, ListedItem, ListedMemberError, listed_members, parse_listed};

    #[test]
    fn every_member_is_read_trimmed_in_written_order() {
        let members = listed_members(" a ,b,  c", ListedItem::AttributeName).collect::<Result<Vec<_>, _>>();
        pretty_assertions::assert_eq!(members, Ok(vec!["a", "b", "c"]));
        pretty_assertions::assert_eq!(parse_listed::<u8>("1, 2", ListedItem::AggregationMethod).expect("two numbers"), vec![1, 2]);
    }

    /// Two commas, a leading or a trailing one, a blank member and an empty
    /// list each leave a member with no text.
    #[test]
    fn an_empty_member_is_refused_naming_the_list() {
        for raw in ["a,,b", ",a", "a,", "a, ,b", ""] {
            assert!(
                matches!(
                    parse_listed::<String>(raw, ListedItem::DatasetIdentifier),
                    Err(ListedMemberError::List(CommaSeparatedListError::EmptyMember {
                        list: ListedItem::DatasetIdentifier
                    }))
                ),
                "{raw:?}"
            );
        }
    }

    #[test]
    fn a_member_the_element_refuses_is_refused_with_its_reason() {
        let refused = parse_listed::<u8>("1,x", ListedItem::OrderingTerm);
        assert!(matches!(refused, Err(ListedMemberError::Member(_))), "{refused:?}");
    }

    #[test]
    fn an_empty_member_names_what_the_list_enumerates() {
        for (list, text) in [
            (ListedItem::EntityIdentifier, "entity identifier list contains an empty member"),
            (ListedItem::AttributeName, "attribute name list contains an empty member"),
            (ListedItem::DatasetIdentifier, "dataset identifier list contains an empty member"),
            (ListedItem::AggregationMethod, "aggregation method list contains an empty member"),
            (ListedItem::OrderingTerm, "ordering term list contains an empty member"),
            (ListedItem::OptionsToken, "options token list contains an empty member"),
        ] {
            pretty_assertions::assert_eq!(CommaSeparatedListError::EmptyMember { list }.to_string(), text);
        }
    }
}
