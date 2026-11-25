//! Why a name a request writes is no name: its spelling after a prefix
//! breaks the clause 4.6.2 grammar, or the request's context expands it to no
//! IRI, which JSON-LD expansion would drop or keep as a name that reads back
//! as another.

use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, coerced_term::CoercedTerm, entity_type_name::EntityTypeName};
use thiserror::Error;

/// Why a written name is refused (clause 4.6.2).
#[derive(Debug, Error)]
pub enum NameRefusal {
    /// An attribute name the request's context reads as a compact IRI breaks
    /// the clause 4.6.2 grammar after its colon.
    #[error("'{attribute}' is not an attribute name: the part after the prefix must start with a letter and hold only letters, digits and '_'")]
    PrefixedAttributeName {
        /// The name, as the request wrote it.
        attribute: AttributeName,
    },
    /// An attribute name the request's context expands to no IRI, so that
    /// expansion would drop the attribute it names (clause 4.6.2).
    #[error("'{attribute}' is not an attribute name: it expands to no absolute IRI under the request's @context")]
    UnexpandedAttributeName {
        /// The name, as the request wrote it.
        attribute: AttributeName,
    },
    /// An `objectType` the request's context reads as a compact IRI breaks
    /// the clause 4.6.2 grammar of an entity type name after its colon.
    #[error(
        "attribute '{attribute}': '{object_type}' is not an entity type name: the part after the prefix must start with a letter and hold only letters, digits and '_'"
    )]
    PrefixedObjectType {
        /// The attribute whose instance writes the `objectType`.
        attribute: AttributeName,
        /// The `objectType`, as the request wrote it.
        object_type: CoercedTerm,
    },
    /// An entity type the request's context reads as a compact IRI breaks the
    /// clause 4.6.2 grammar after its colon.
    #[error("'{entity_type}' is not an entity type name: the part after the prefix must start with a letter and hold only letters, digits and '_'")]
    PrefixedEntityType {
        /// The type, as the request wrote it.
        entity_type: EntityTypeName,
    },
}
