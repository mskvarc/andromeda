//! Delete Attribute (clause 5.6.5).

use crate::{
    entity_state::EntityState,
    operation::{
        attribute_write::{InstanceRemovalOutcome, remove_instance},
        mutation_error::MutationError,
        selection::DatasetSelection,
    },
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier};

impl EntityState {
    /// Delete instances of `attribute` (clause 5.6.5).
    ///
    /// [`DatasetSelection::All`] removes the attribute with every instance;
    /// [`DatasetSelection::Single`] removes only the instance with that
    /// `datasetId`; [`DatasetSelection::Default`] removes the instance with
    /// none. The attribute goes with its last instance.
    ///
    /// # Errors
    ///
    /// Returns [`MutationError::AttributeNotFound`] when the entity has no
    /// such attribute, and [`MutationError::DatasetNotFound`] or
    /// [`MutationError::DefaultInstanceNotFound`] when the selected instance
    /// is absent.
    pub fn delete_attribute(&mut self, attribute: &AttributeName, selection: DatasetSelection<'_>) -> Result<(), MutationError> {
        if !self.attributes().contains(attribute) {
            return Err(MutationError::AttributeNotFound {
                attribute: attribute.clone(), // clone: the error outlives the borrowed request.
            });
        }
        let dataset_id: Option<&Identifier> = match selection {
            DatasetSelection::All => {
                self.attributes_mut().remove(attribute);
                return Ok(());
            }
            DatasetSelection::Single(dataset_id) => Some(dataset_id),
            DatasetSelection::Default => None,
        };
        match remove_instance(self.attributes_mut(), attribute, dataset_id) {
            InstanceRemovalOutcome::Removed => Ok(()),
            InstanceRemovalOutcome::Absent => Err(MutationError::missing_instance(attribute, dataset_id)),
        }
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::operation::{
        mutation_error::MutationError,
        selection::DatasetSelection,
        speed_entity::{SPEED, dataset, entity, instances, name, number, property, values, with_three_instances},
    };

    #[test]
    fn delete_attr_removes_key() {
        let mut target = entity(vec![("https://example.com/a", instances(vec![(None, property(number(1)))]))]);
        target
            .delete_attribute(&name("https://example.com/a"), DatasetSelection::All)
            .expect("delete succeeds");
        assert!(!target.attributes().contains("https://example.com/a"));
    }

    #[test]
    fn delete_attr_not_found() {
        let mut target = entity(vec![("https://example.com/a", instances(vec![(None, property(number(1)))]))]);
        assert!(target.delete_attribute(&name("https://example.com/missing"), DatasetSelection::All).is_err());
    }

    #[test]
    fn deleting_all_instances_removes_every_dataset() {
        let mut target = with_three_instances();
        target.delete_attribute(&name(SPEED), DatasetSelection::All).expect("delete succeeds");
        assert!(!target.attributes().contains(SPEED));
    }

    #[test]
    fn deleting_a_single_instance_removes_only_that_dataset() {
        let mut target = with_three_instances();
        target
            .delete_attribute(&name(SPEED), DatasetSelection::Single(&dataset("urn:ngsi-ld:Dataset:A")))
            .expect("delete succeeds");
        pretty_assertions::assert_eq!(values(&target, SPEED), vec![20, 30]);
    }

    #[test]
    fn deleting_the_default_instance_leaves_the_datasets_alone() {
        let mut target = with_three_instances();
        target.delete_attribute(&name(SPEED), DatasetSelection::Default).expect("delete succeeds");
        pretty_assertions::assert_eq!(values(&target, SPEED), vec![10, 20]);
    }

    #[test]
    fn deleting_an_absent_attribute_is_a_typed_error() {
        let mut target = entity(vec![("https://example.com/other", instances(vec![(None, property(number(1)))]))]);
        let error = target.delete_attribute(&name(SPEED), DatasetSelection::All).expect_err("must fail");
        assert!(
            matches!(&error, MutationError::AttributeNotFound { attribute } if attribute.as_str() == SPEED),
            "{error:?}"
        );
        pretty_assertions::assert_eq!(error.to_string(), "attribute https://example.com/speed not found");
    }

    #[test]
    fn deleting_an_absent_dataset_is_a_typed_error() {
        let mut target = with_three_instances();
        let error = target
            .delete_attribute(&name(SPEED), DatasetSelection::Single(&dataset("urn:ngsi-ld:Dataset:Z")))
            .expect_err("must fail");
        assert!(
            matches!(&error, MutationError::DatasetNotFound { attribute, dataset_id } if attribute.as_str() == SPEED && dataset_id.as_str() == "urn:ngsi-ld:Dataset:Z"),
            "{error:?}"
        );
    }

    #[test]
    fn deleting_an_absent_default_instance_is_a_typed_error() {
        let mut target = entity(vec![(SPEED, instances(vec![(Some("urn:ngsi-ld:Dataset:A"), property(number(1)))]))]);
        let error = target.delete_attribute(&name(SPEED), DatasetSelection::Default).expect_err("must fail");
        pretty_assertions::assert_eq!(error.to_string(), "attribute https://example.com/speed default instance not found");
    }
}
