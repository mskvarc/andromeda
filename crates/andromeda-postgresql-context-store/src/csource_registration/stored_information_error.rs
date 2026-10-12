//! What a set of `registration_information` rows that no write produces looks
//! like.
//!
//! The broker writes a registration's rows from a validated `information` in
//! one statement, so each variant is a row set something other than that
//! statement left: a restore from another release, or manual surgery. The
//! table's checks make most of them unreachable; assembly still refuses them
//! rather than inventing an `information` the client never wrote.

use andromeda_ngsi_ld_lexicon::{identifier::IdentifierParseError, reference::NgsiLdReferenceError};
use andromeda_ngsi_ld_query::error::QueryError;
use andromeda_postgresql_codes::registry::UnknownCompactCode;
use andromeda_postgresql_query::attribute_name_key::AttributeNameKey;
use thiserror::Error;

/// A `registration_information` row set that does not assemble to an
/// `information`.
#[derive(Debug, Error)]
pub enum StoredInformationError {
    /// The rows skip or repeat a position: items are numbered from 0, and so
    /// are the rows of each item.
    #[error("registration information has no row at item {item_ordinal}, selector {selector_ordinal}")]
    OrdinalGap {
        /// The item position the next row should have had.
        item_ordinal: usize,
        /// The selector position the next row should have had.
        selector_ordinal: usize,
    },
    /// Two rows of one item carry different `propertyNames` or
    /// `relationshipNames`.
    #[error("the rows of registration information item {item_ordinal} disagree on its attribute names")]
    NamesDisagree {
        /// The item whose rows disagree.
        item_ordinal: i32,
    },
    /// An item has a row without entity columns beside other rows, or a row
    /// with entity columns beside one without.
    #[error("registration information item {item_ordinal} mixes a row without entities with selector rows")]
    MixedItem {
        /// The item whose rows mix the two shapes.
        item_ordinal: i32,
    },
    /// An item declares no `entities`, no `propertyNames` and no
    /// `relationshipNames` (Table 5.2.10-1 requires one).
    #[error("registration information item {item_ordinal} declares nothing")]
    EmptyItem {
        /// The empty item.
        item_ordinal: i32,
    },
    /// An item's `propertyNames` or `relationshipNames` is an empty list
    /// (Table 5.2.10-1 forbids one).
    #[error("registration information item {item_ordinal} holds an empty attribute name list")]
    EmptyNames {
        /// The item holding the empty list.
        item_ordinal: i32,
    },
    /// A row's entity type names and its shape code disagree: one is absent
    /// without the other, the single shape holds other than one name, or the
    /// array shape holds none.
    #[error("registration information item {item_ordinal}, selector {selector_ordinal} has entity types that do not fit their shape")]
    TypeShape {
        /// The item of the row.
        item_ordinal: i32,
        /// The selector of the row.
        selector_ordinal: i32,
    },
    /// A row's shape code is a number the registry never assigned.
    #[error("a stored registration information type shape is not a known code: {0}")]
    TypeShapeCode(#[source] UnknownCompactCode),
    /// An `attribute_names` key the dictionary holds no name for.
    #[error("a stored registration information name key {} has no attribute name", .0.get())]
    UnknownNameKey(AttributeNameKey),
    /// An entity type name that is not a type name.
    #[error("a stored registration information entity type is not a type name: {0}")]
    EntityType(#[source] NgsiLdReferenceError),
    /// An entity identifier that is not a URI.
    #[error("a stored registration information entity id is not a URI: {0}")]
    EntityId(#[source] IdentifierParseError),
    /// An identifier pattern that does not compile.
    #[error("a stored registration information idPattern is not a pattern: {0}")]
    IdPattern(#[source] Box<QueryError>),
}
