//! Which instance of an attribute a matcher reads: the one of the dataset it names, or the attribute's one instance
//! (clause 4.5.5).

use ngsi_ld::model::{Attribute, DatasetId, Instances, Output};

/// Why an attribute holds no instance a matcher can read.
#[derive(Debug, thiserror::Error)]
pub(crate) enum MissingInstance<'a> {
    /// No instance is the default one.
    #[error("has no default instance")]
    Default,
    /// No instance carries the dataset the matcher reads.
    #[error("has no instance of dataset {0}")]
    Dataset(&'a DatasetId),
    /// The matcher names no dataset and the attribute holds this many instances.
    #[error("has {0} instances")]
    Several(usize),
}

/// The instance of `instances` of the dataset `dataset_id`, or the one instance when it names none; or why there is
/// none.
pub(crate) fn read_instance<'a>(
    dataset_id: Option<&'a DatasetId>,
    instances: &'a Instances<Attribute<Output>>,
) -> Result<&'a Attribute<Output>, MissingInstance<'a>> {
    match dataset_id {
        // The default instance is written without a `datasetId` (clause 4.5.5).
        Some(DatasetId::DefaultInstance) => instances
            .iter()
            .find(|instance| {
                instance
                    .meta()
                    .dataset_id
                    .as_ref()
                    .is_none_or(|dataset_id| *dataset_id == DatasetId::DefaultInstance)
            })
            .ok_or(MissingInstance::Default),
        Some(dataset_id) => instances
            .iter()
            .find(|instance| instance.meta().dataset_id.as_ref() == Some(dataset_id))
            .ok_or(MissingInstance::Dataset(dataset_id)),
        None if instances.len() == 1 => Ok(instances.first()),
        None => Err(MissingInstance::Several(instances.len())),
    }
}
