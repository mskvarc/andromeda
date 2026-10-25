//! The instances the written instances of one attribute name (clause 4.5.5.1).
//!
//! "There can only be one default Attribute instance for an Attribute with a
//! given Attribute name in any request or response", and named instances are
//! told apart by their `datasetId`. A write lands its instances one by one on
//! the held instance with the same identity, so two written instances naming
//! one would land on it twice. Every Core write refuses them alike: the
//! admission of a written document, and each path that lands written
//! sub-attributes, whether onto held ones or as new ones.

use crate::{attribute_instances::RepeatedInstance, document::instance_document::InstanceDocument, refusal::EntityRefusal};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier};
use std::collections::HashSet;

/// Refuse the written instances of the attribute `name` when two of them name
/// one instance.
///
/// # Errors
///
/// Returns [`EntityRefusal::MultipleDefaultInstances`] or
/// [`EntityRefusal::RepeatedInstance`] for the first identity named twice.
pub(crate) fn admit_distinct_identities(name: &AttributeName, instances: &[InstanceDocument]) -> Result<(), EntityRefusal> {
    match repeated_identity(instances) {
        Some(repeated) => Err(EntityRefusal::repeated_instance(name, repeated)),
        None => Ok(()),
    }
}

/// The first identity two of `instances` name, deletions among them.
fn repeated_identity(instances: &[InstanceDocument]) -> Option<RepeatedInstance> {
    let mut seen: HashSet<Option<&Identifier>> = HashSet::with_capacity(instances.len());
    instances
        .iter()
        .map(InstanceDocument::dataset_id)
        .find(|dataset_id| !seen.insert(*dataset_id))
        .map(|dataset_id| match dataset_id {
            // clone: the refusal outlives the borrowed request.
            Some(dataset_id) => RepeatedInstance::Dataset(dataset_id.clone()),
            None => RepeatedInstance::Default,
        })
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        document::{instance_document::InstanceDocument, instance_identities::admit_distinct_identities},
        refusal::EntityRefusal,
    };
    use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier};

    fn name() -> AttributeName {
        "https://example.org/accuracy".parse().expect("an attribute name")
    }

    fn instance(dataset_id: Option<&str>) -> InstanceDocument {
        InstanceDocument::builder()
            .dataset_id(dataset_id.map(|raw| raw.parse::<Identifier>().expect("a dataset identifier")))
            .build()
    }

    #[test]
    fn distinct_identities_are_admitted() {
        let instances = [instance(None), instance(Some("urn:ngsi-ld:Dataset:a")), instance(Some("urn:ngsi-ld:Dataset:b"))];
        assert!(admit_distinct_identities(&name(), &instances).is_ok());
    }

    #[test]
    fn a_repeated_identity_is_refused_by_what_it_repeats() {
        let defaults = [instance(None), instance(Some("urn:ngsi-ld:Dataset:a")), instance(None)];
        assert!(matches!(
            admit_distinct_identities(&name(), &defaults),
            Err(EntityRefusal::MultipleDefaultInstances { .. })
        ));
        let named = [instance(Some("urn:ngsi-ld:Dataset:a")), instance(Some("urn:ngsi-ld:Dataset:a"))];
        assert!(matches!(
            admit_distinct_identities(&name(), &named),
            Err(EntityRefusal::RepeatedInstance { ref dataset_id, .. }) if dataset_id.as_str() == "urn:ngsi-ld:Dataset:a"
        ));
    }
}
