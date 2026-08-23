//! Direct implementation of entity policy's historical-dataset port.

use crate::repo::TemporalRepository;
use andromeda_context_space::context_target::ContextTarget;
use andromeda_entity::{error::EntityError, port::attribute_history::AttributeDatasetHistory};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, uri::Uri};
use async_trait::async_trait;

#[async_trait]
impl AttributeDatasetHistory for TemporalRepository {
    async fn dataset_identifiers(&self, target: &ContextTarget, entity: &Uri, attribute: &AttributeName) -> Result<Vec<Option<Uri>>, EntityError> {
        TemporalRepository::attribute_dataset_ids(self, target, entity, attribute)
            .await
            .map_err(|error| EntityError::persistence(error.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use crate::repo::TemporalRepository;
    use andromeda_entity::port::attribute_history::AttributeDatasetHistory;

    fn assert_attribute_history<T: AttributeDatasetHistory>() {}

    #[test]
    fn temporal_repository_directly_implements_entity_attribute_history() {
        assert_attribute_history::<TemporalRepository>();
    }
}
