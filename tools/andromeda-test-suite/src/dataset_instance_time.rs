//! The system times (`createdAt`, `modifiedAt`, clause 4.8) of one dataset instance of an attribute, read from an
//! answer so a later read can be held to them while another instance of the same attribute moves.

use crate::system_time::SystemMember;
use ngsi_ld::model::{Attribute, AttributeName, DatasetId, DateTime, Entity, Output};

/// The entity holds no instance of the attribute under that dataset, or the instance carries no such time.
#[derive(Debug, thiserror::Error)]
#[error("{name} of the entity holds no instance of dataset {dataset} carrying {member}")]
pub struct DatasetInstanceTimeNotFound {
    /// The attribute name.
    name: AttributeName,
    /// The dataset looked for.
    dataset: DatasetId,
    /// The system member read.
    member: SystemMember,
}

/// The `member` of the instance of attribute `name` of `entity` that has dataset `dataset`.
///
/// # Errors
///
/// Returns [`DatasetInstanceTimeNotFound`] when the entity holds no such instance or it carries no such time.
pub fn of_dataset_instance(
    entity: &Entity<Output>,
    name: &AttributeName,
    dataset: &DatasetId,
    member: SystemMember,
) -> Result<DateTime, DatasetInstanceTimeNotFound> {
    entity
        .attributes
        .get(name)
        .into_iter()
        .flat_map(|instances| instances.iter())
        .find(|instance| instance.meta().dataset_id.as_ref() == Some(dataset))
        .and_then(|instance| time_of(instance, member))
        .ok_or_else(|| DatasetInstanceTimeNotFound {
            // The error owns what it names; the caller keeps its borrowed name and dataset.
            name: name.clone(),
            dataset: dataset.clone(),
            member,
        })
}

/// The `member` of `instance`.
const fn time_of(instance: &Attribute<Output>, member: SystemMember) -> Option<DateTime> {
    let output = &instance.meta().output;
    match member {
        SystemMember::CreatedAt => output.created_at,
        SystemMember::ModifiedAt => output.modified_at,
        SystemMember::DeletedAt => output.deleted_at,
    }
}

#[cfg(test)]
mod tests {
    use crate::{dataset_instance_time::of_dataset_instance, system_time::SystemMember};
    use googletest::prelude::*;
    use ngsi_ld::model::{AttributeName, DatasetId, DateTime, Entity, Lossless, Output};

    fn entity() -> Result<Entity<Output>> {
        Ok(Entity::<Output>::parse(
            r#"{"id":"urn:ngsi-ld:V:1","type":"V","name":[
            {"type":"Property","value":"a","datasetId":"urn:ngsi-ld:Dataset:A","createdAt":"2024-01-01T00:00:00Z","modifiedAt":"2024-01-03T00:00:00Z"},
            {"type":"Property","value":"b","datasetId":"urn:ngsi-ld:Dataset:B","createdAt":"2024-01-02T00:00:00Z","modifiedAt":"2024-01-02T00:00:00Z"}]}"#,
            Lossless::Normalized,
        )?)
    }

    #[gtest]
    fn each_dataset_instance_answers_its_own_times() -> Result<()> {
        let entity = entity()?;
        let name = "name".parse::<AttributeName>()?;
        let a = "urn:ngsi-ld:Dataset:A".parse::<DatasetId>()?;
        let b = "urn:ngsi-ld:Dataset:B".parse::<DatasetId>()?;
        expect_that!(
            of_dataset_instance(&entity, &name, &a, SystemMember::ModifiedAt)?,
            eq("2024-01-03T00:00:00Z".parse::<DateTime>()?)
        );
        verify_that!(
            of_dataset_instance(&entity, &name, &b, SystemMember::CreatedAt)?,
            eq("2024-01-02T00:00:00Z".parse::<DateTime>()?)
        )
    }

    #[gtest]
    fn a_dataset_without_an_instance_is_an_error() -> Result<()> {
        let entity = entity()?;
        let c = "urn:ngsi-ld:Dataset:C".parse::<DatasetId>()?;
        let name = "name".parse::<AttributeName>()?;
        verify_that!(of_dataset_instance(&entity, &name, &c, SystemMember::CreatedAt), err(anything()))
    }
}
