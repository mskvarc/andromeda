//! Replace Attribute (clause 5.6.19).

use crate::{
    document::instance_document::InstanceDocument,
    entity_state::EntityState,
    operation::{instance_patch::new_instance, mutation_error::MutationError, selection::addressed_dataset},
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier};

impl EntityState {
    /// Replace one instance of `attribute` with `replacement` (clause
    /// 5.6.19).
    ///
    /// The instance replaced is the one [`addressed_dataset`] resolves from the
    /// `dataset_id` URL parameter and the replacement's own `datasetId`; when
    /// neither names one, it is the default instance (clause 4.5.5). The
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
    pub fn replace_attribute(
        &mut self,
        attribute: &AttributeName,
        dataset_id: Option<&Identifier>,
        replacement: &InstanceDocument,
    ) -> Result<(), MutationError> {
        let Some(held) = self.attributes().get(attribute) else {
            return Err(MutationError::AttributeNotFound {
                attribute: attribute.clone(), // clone: the error outlives the borrowed request.
            });
        };
        let addressed = addressed_dataset(dataset_id, replacement);
        if !held.contains(addressed) {
            return Err(MutationError::missing_instance(attribute, addressed));
        }
        let instance = new_instance(attribute, replacement, None)?;
        if let Some(held) = self.attributes_mut().get_mut(attribute) {
            // clone: the instance is keyed by its own copy of the addressed identity.
            held.insert(addressed.cloned(), instance);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::operation::{
        mutation_error::MutationError,
        speed_entity::{SPEED, dataset, name, number, property_document, values, with_three_instances},
    };

    #[test]
    fn replacing_an_absent_dataset_is_a_typed_error() {
        let mut target = with_three_instances();
        let error = target
            .replace_attribute(&name(SPEED), Some(&dataset("urn:ngsi-ld:Dataset:Z")), &property_document(number(99), None))
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

    /// Clause 5.6.19: the addressed instance is replaced where it stands and
    /// the other instances are untouched.
    #[test]
    fn replacing_an_instance_keeps_its_place_and_its_siblings() {
        let mut target = with_three_instances();
        target
            .replace_attribute(&name(SPEED), Some(&dataset("urn:ngsi-ld:Dataset:B")), &property_document(number(99), None))
            .expect("the replacement applies");
        pretty_assertions::assert_eq!(values(&target, SPEED), vec![10, 99, 30]);
    }

    /// Without a URL `datasetId` the replacement's own one addresses the
    /// instance (clause 5.6.19.4).
    #[test]
    fn the_replacements_own_dataset_id_addresses_without_a_url_one() {
        let mut target = with_three_instances();
        target
            .replace_attribute(&name(SPEED), None, &property_document(number(99), Some("urn:ngsi-ld:Dataset:A")))
            .expect("the replacement applies");
        pretty_assertions::assert_eq!(values(&target, SPEED), vec![99, 20, 30]);
    }

    #[test]
    fn replacing_an_absent_attribute_is_a_typed_error() {
        let mut target = with_three_instances();
        let error = target
            .replace_attribute(&name("https://example.com/other"), None, &property_document(number(99), None))
            .expect_err("must fail");
        assert!(matches!(error, MutationError::AttributeNotFound { .. }), "{error:?}");
    }
}
