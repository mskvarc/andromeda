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

    /// Drop `createdAt` and `modifiedAt` from every instance of this member,
    /// its own sub-attributes left as they are.
    pub fn forget_system_instants(&mut self) {
        match self {
            AttributeMember::Remove => {}
            AttributeMember::Instances(instances) => instances.iter_mut().for_each(InstanceDocument::forget_system_instants),
        }
    }

    /// Drop `createdAt` and `modifiedAt` from every instance of this member
    /// and of its sub-attributes, at every depth.
    pub fn forget_system_instants_throughout(&mut self) {
        match self {
            AttributeMember::Remove => {}
            AttributeMember::Instances(instances) => instances.iter_mut().for_each(|instance| {
                instance.forget_system_instants();
                instance
                    .sub_attributes_mut()
                    .values_mut()
                    .for_each(AttributeMember::forget_system_instants_throughout);
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        document::{attribute_member::AttributeMember, instance_document::InstanceDocument},
        system_instant::SystemInstant,
    };
    use indexmap::IndexMap;

    /// A client-written instant.
    fn at() -> SystemInstant {
        "1900-01-01T00:00:00Z".parse().expect("RFC 3339")
    }

    /// A member of one instance carrying both system times, with one
    /// sub-attribute that carries them too.
    fn stamped_member() -> AttributeMember {
        let nested = InstanceDocument::builder().created_at(Some(at())).modified_at(Some(at())).build();
        let instance = InstanceDocument::builder()
            .created_at(Some(at()))
            .modified_at(Some(at()))
            .sub_attributes(IndexMap::from([(
                "https://example.org/method".parse().expect("a valid name"),
                AttributeMember::Instances(vec![nested]),
            )]))
            .build();
        AttributeMember::Instances(vec![instance])
    }

    /// The system times of the member's instance and of its one sub-attribute.
    fn times(member: &AttributeMember) -> [(Option<SystemInstant>, Option<SystemInstant>); 2] {
        let instance = &member.instances()[0];
        let nested = &instance.sub_attributes()[0].instances()[0];
        [(instance.created_at(), instance.modified_at()), (nested.created_at(), nested.modified_at())]
    }

    #[test]
    fn forgetting_system_times_leaves_the_sub_attributes_alone() {
        let mut member = stamped_member();
        member.forget_system_instants();
        pretty_assertions::assert_eq!(times(&member), [(None, None), (Some(at()), Some(at()))]);
    }

    #[test]
    fn forgetting_system_times_throughout_reaches_every_depth() {
        let mut member = stamped_member();
        member.forget_system_instants_throughout();
        pretty_assertions::assert_eq!(times(&member), [(None, None), (None, None)]);
    }

    #[test]
    fn a_removal_has_no_system_times_to_forget() {
        let mut member = AttributeMember::Remove;
        member.forget_system_instants_throughout();
        pretty_assertions::assert_eq!(member, AttributeMember::Remove);
    }
}
