//! One written attribute member: its instances, or its removal.
//!
//! Clause 5.5.12 deletes "the complete member" when a merge fragment sets the
//! member itself to the NGSI-LD Null, every instance and every dataset with
//! it; that is not the deletion of one instance by its family's deletion
//! encoding, which an instance of [`AttributeMember::Instances`] carries.

use crate::document::instance_document::InstanceDocument;

/// A written attribute member.
#[derive(Clone, Debug, PartialEq)]
pub enum AttributeMember {
    /// The member is the NGSI-LD Null: remove the whole attribute.
    Remove,
    /// The written instances, in order. A Temporal Evolution fragment may hold
    /// several instances of one `datasetId`.
    Instances(Vec<InstanceDocument>),
}

impl AttributeMember {
    /// The written instances; none for a removal.
    #[must_use]
    pub fn instances(&self) -> &[InstanceDocument] {
        match self {
            AttributeMember::Remove => &[],
            AttributeMember::Instances(instances) => instances,
        }
    }
}
