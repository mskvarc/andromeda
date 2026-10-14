//! The refusal of a comma-separated list parameter whose member is empty.
//!
//! Entity identifiers, attribute names and dataset identifiers travel as one
//! comma-separated string in a query string or a query body member. An empty
//! member is malformed input in each of them, so the refusal is one concept
//! and one type shared by every reader of such a list.

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

#[cfg(test)]
mod tests {
    use crate::comma_separated_list::{CommaSeparatedListError, ListedItem};

    #[test]
    fn an_empty_member_names_what_the_list_enumerates() {
        for (list, text) in [
            (ListedItem::EntityIdentifier, "entity identifier list contains an empty member"),
            (ListedItem::AttributeName, "attribute name list contains an empty member"),
            (ListedItem::DatasetIdentifier, "dataset identifier list contains an empty member"),
        ] {
            pretty_assertions::assert_eq!(CommaSeparatedListError::EmptyMember { list }.to_string(), text);
        }
    }
}
