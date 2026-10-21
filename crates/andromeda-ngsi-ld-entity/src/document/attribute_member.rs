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

    /// Drop the output-only members of every instance of this member and of
    /// its sub-attributes, at every depth
    /// ([`InstanceDocument::forget_output_only_members`]).
    pub fn forget_output_only_members_throughout(&mut self) {
        match self {
            AttributeMember::Remove => {}
            AttributeMember::Instances(instances) => instances.iter_mut().for_each(|instance| {
                instance.forget_output_only_members();
                instance
                    .sub_attributes_mut()
                    .values_mut()
                    .for_each(AttributeMember::forget_output_only_members_throughout);
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::document::{attribute_member::AttributeMember, instance_document::InstanceDocument};
    use andromeda_ngsi_ld_lexicon::{
        attribute_name::AttributeName,
        identifier::Identifier,
        time::{date_time::NgsiLdDateTime, system_instant::SystemInstant},
    };
    use indexmap::IndexMap;

    /// A client-written instant.
    fn at() -> SystemInstant {
        "1900-01-01T00:00:00Z".parse().expect("RFC 3339")
    }

    /// An instance carrying every output-only member a producer could write.
    fn claiming(sub_attributes: IndexMap<AttributeName, AttributeMember>) -> InstanceDocument {
        InstanceDocument::builder()
            .created_at(Some(at()))
            .modified_at(Some(at()))
            .deleted_at(Some("1900-01-02T00:00:00Z".parse::<NgsiLdDateTime>().expect("a DateTime")))
            .instance_id(Some("urn:ngsi-ld:instance:1".parse::<Identifier>().expect("a URI")))
            .sub_attributes(sub_attributes)
            .build()
    }

    /// A member of one instance carrying every output-only member, with one
    /// sub-attribute that carries them too.
    fn claiming_member() -> AttributeMember {
        let nested = claiming(IndexMap::new());
        AttributeMember::Instances(vec![claiming(IndexMap::from([(
            "https://example.org/method".parse().expect("a valid name"),
            AttributeMember::Instances(vec![nested]),
        )]))])
    }

    /// Whether `instance` still carries an output-only member.
    fn claims_any(instance: &InstanceDocument) -> bool {
        instance.created_at().is_some() || instance.modified_at().is_some() || instance.deleted_at().is_some() || instance.instance_id().is_some()
    }

    /// Table 5.2.5-2: `createdAt`, `modifiedAt`, `deletedAt` and `instanceId`
    /// are output-only, and a broker ignores them when a producer writes them.
    #[test]
    fn forgetting_output_only_members_throughout_reaches_every_depth() {
        let mut member = claiming_member();
        member.forget_output_only_members_throughout();
        let instance = &member.instances()[0];
        let nested = &instance.sub_attributes()[0].instances()[0];
        assert!(!claims_any(instance) && !claims_any(nested), "{member:?}");
    }

    #[test]
    fn a_removal_has_no_output_only_members_to_forget() {
        let mut member = AttributeMember::Remove;
        member.forget_output_only_members_throughout();
        pretty_assertions::assert_eq!(member, AttributeMember::Remove);
    }
}
