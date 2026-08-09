//! Direct implementation of entity policy's historical-dataset port.

use crate::recorder::TemporalRecorder;
use andromeda_entity::{error::EntityError, port::attribute_history::AttributeDatasetHistory};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, uri::Uri};
use async_trait::async_trait;

#[async_trait]
impl AttributeDatasetHistory for TemporalRecorder {
    async fn dataset_identifiers(&self, entity: &Uri, attribute: &AttributeName) -> Result<Vec<Option<Uri>>, EntityError> {
        self.get_teas_for_entity_attr(entity, attribute)
            .await
            .map(|entries| entries.into_iter().map(|entry| entry.dataset_id).collect())
            .map_err(|error| EntityError::persistence(error.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use crate::recorder::TemporalRecorder;
    use andromeda_entity::port::attribute_history::AttributeDatasetHistory;

    fn assert_attribute_history<T: AttributeDatasetHistory>() {}

    #[test]
    fn temporal_recorder_directly_implements_entity_attribute_history() {
        assert_attribute_history::<TemporalRecorder>();
    }
}
