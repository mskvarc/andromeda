//! One row of the temporal attribute catalogue.

use crate::value_type::ValueType;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, attribute_type::AttributeType, uri::Uri};

/// A typed row from the `temporal_entity_attributes` table.
#[derive(Debug, Clone)]
pub struct TeaEntry {
    /// Expanded IRI of the attribute this row belongs to.
    pub attribute_name: AttributeName,
    /// Attribute family the recorded instances belong to.
    pub attribute_type: AttributeType,
    /// Multi-attribute discriminator (NGSI-LD 5.2.5).
    ///
    /// Typed as a URI rather than a URN: the specification constrains
    /// `datasetId` to a URI, and plain-URI dataset identifiers are legal. The
    /// previous `Urn` typing silently discarded them, collapsing distinct
    /// instances onto the default one.
    pub dataset_id: Option<Uri>,
    /// Storage column the recorded values live in.
    pub value_type: ValueType,
}
