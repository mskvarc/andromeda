//! The identity of one attribute instance: its expanded attribute name and its
//! `datasetId` (clause 4.5.5).

use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier};

/// Identity of one attribute instance inside an entity: the expanded attribute
/// name together with its multi-attribute discriminator (clause 5.2.5).
///
/// This is the grain at which instances are tracked: what a change, a removal
/// or a temporal history row is about.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct AttributeInstanceKey {
    /// The expanded attribute name.
    pub attr: AttributeName,
    /// The `datasetId`; `None` for the default instance.
    pub dataset_id: Option<Identifier>,
}
