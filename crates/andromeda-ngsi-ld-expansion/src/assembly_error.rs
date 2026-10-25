//! Why an expanded tree is not assembled into the written document.
//!
//! Assembly refuses what the model does not admit, which clause 5.5.4 makes a
//! `BadRequestData`, and can meet one fault of its own: a geometry held as the
//! value of a plain Property is written back as numbers, and an ordinate that
//! has no JSON number is the broker's fault, an `InternalError` under clause
//! 5.5.6, since every ordinate a geometry holds is finite.

use andromeda_ngsi_ld_entity::refusal::EntityRefusal;
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_number::JsonNumberError;
use thiserror::Error;

/// A geometry ordinate the broker holds that has no JSON number.
#[derive(Debug, Error)]
#[error("attribute '{attribute}': a geometry ordinate has no JSON number: {source}")]
pub struct UnspellableOrdinate {
    /// The attribute whose value holds the geometry, by its expanded IRI.
    pub attribute: AttributeName,
    /// The conversion's own failure.
    #[source]
    pub source: JsonNumberError,
}

/// Why an expanded tree is not assembled.
#[derive(Debug, Error)]
pub enum AssemblyError {
    /// The document is not one the model admits (clause 5.5.4).
    #[error(transparent)]
    Refused(#[from] EntityRefusal),
    /// An ordinate of a geometry held as a Property value has no JSON number.
    #[error(transparent)]
    UnspellableOrdinate(#[from] UnspellableOrdinate),
}
