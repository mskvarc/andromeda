//! Replace Attribute (clause 5.6.19).

use crate::{
    document::instance_document::InstanceDocument,
    entity_state::EntityState,
    operation::{instance_patch::new_instance, mutation_error::MutationError},
};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;

impl EntityState {
    /// Replace one instance of `attribute` with `replacement` (clause
    /// 5.6.19).
    ///
    /// The instance replaced is the one the replacement's own `datasetId`
    /// names (Table 6.7.3.3-1 defines no URL parameter for it); when it names
    /// none, it is the default instance (clause 4.5.5). The
    /// replacement takes the replaced instance's place among the others. Its
    /// `createdAt` is carried over afterwards by
    /// [`EntityState::stamp_attribute_instants`], as clause 5.6.19.4 keeps it.
    ///
    /// # Errors
    ///
    /// Returns [`MutationError::AttributeNotFound`] when the entity has no
    /// such attribute, [`MutationError::DatasetNotFound`] or
    /// [`MutationError::DefaultInstanceNotFound`] when no instance is
    /// addressed, and [`MutationError::Refused`] when the replacement cannot
    /// be state.
    pub fn replace_attribute(&mut self, attribute: &AttributeName, replacement: &InstanceDocument) -> Result<(), MutationError> {
        let Some(held) = self.attributes().get(attribute) else {
            return Err(MutationError::AttributeNotFound {
                attribute: attribute.clone(), // clone: the error outlives the borrowed request.
            });
        };
        let addressed = replacement.dataset_id();
        if !held.contains(addressed) {
            return Err(MutationError::missing_instance(attribute, addressed));
        }
        let instance = new_instance(attribute, replacement, None)?;
        if let Some(held) = self.attributes_mut().get_mut(attribute) {
            held.insert(addressed.cloned(), instance);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::operation::{
        mutation_error::MutationError,
        speed_entity::{SPEED, name, number, property_document, values, with_three_instances},
    };

    #[test]
    fn replacing_an_absent_dataset_is_a_typed_error() {
        let mut target = with_three_instances();
        let error = target
            .replace_attribute(&name(SPEED), &property_document(number(99), Some("urn:ngsi-ld:Dataset:Z")))
            .expect_err("must fail");
        assert!(
            matches!(&error, MutationError::DatasetNotFound { attribute, dataset_id } if attribute.as_str() == SPEED && dataset_id.as_str() == "urn:ngsi-ld:Dataset:Z"),
            "{error:?}"
        );
        pretty_assertions::assert_eq!(
            error.to_string(),
            "attribute https://example.com/speed with datasetId urn:ngsi-ld:Dataset:Z not found"
        );
    }

    /// Clause 5.6.19.4: the instance the replacement's own `datasetId` names is
    /// replaced where it stands and the other instances are untouched.
    #[test]
    fn the_replacements_own_dataset_id_addresses_the_instance_it_replaces_in_place() {
        let mut target = with_three_instances();
        target
            .replace_attribute(&name(SPEED), &property_document(number(99), Some("urn:ngsi-ld:Dataset:B")))
            .expect("the replacement applies");
        pretty_assertions::assert_eq!(values(&target, SPEED), vec![10, 99, 30]);
    }

    #[test]
    fn replacing_an_absent_attribute_is_a_typed_error() {
        let mut target = with_three_instances();
        let error = target
            .replace_attribute(&name("https://example.com/other"), &property_document(number(99), None))
            .expect_err("must fail");
        assert!(matches!(error, MutationError::AttributeNotFound { .. }), "{error:?}");
    }
}
