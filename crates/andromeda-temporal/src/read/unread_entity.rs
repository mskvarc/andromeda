//! An entity of a page whose stored history did not read back.

use crate::error::TemporalError;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;

/// An entity of a page whose stored history did not read back, which a
/// query drops alone while the rest of the page still answers.
#[derive(Debug)]
pub(crate) struct UnreadEntity {
    /// The entity the read was for.
    pub(crate) entity_id: Identifier,
    /// Why its history did not read back, boxed so a page read that
    /// succeeds does not carry the size of a failure.
    pub(crate) error: Box<TemporalError>,
}
