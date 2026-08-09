//! Attribute-scoped temporal-history persistence operations.

use crate::{entry::TeaEntry, error::TemporalStoreError, mapper::row_to_tea, params::DeleteScope, repo::TemporalRepository};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, uri::Uri};

impl TemporalRepository {
    /// Temporal entity attribute entries for a specific attribute.
    pub async fn get_teas_for_entity_attr(&self, entity_id: &Uri, attribute_name: &AttributeName) -> Result<Vec<TeaEntry>, TemporalStoreError> {
        let rows = sqlx::query(
            r#"
            SELECT tea.id, tea.entity_id, an.name AS attribute_name,
                   tea.attribute_type, tea.dataset_id, tea.value_type
            FROM temporal_entity_attributes tea
            JOIN attribute_names an ON an.id = tea.attribute_name_id
            WHERE tea.entity_id = $1 AND an.name = $2
            "#,
        )
        .bind(entity_id.as_str())
        .bind(attribute_name.as_str())
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(row_to_tea).collect()
    }

    /// Delete one or all dataset instances of an attribute.
    pub(crate) async fn delete_tea(
        &self,
        entity_id: &Uri,
        attribute_name: &AttributeName,
        dataset_id: Option<&Uri>,
        scope: DeleteScope,
    ) -> Result<u64, TemporalStoreError> {
        let result = match scope {
            DeleteScope::All => {
                sqlx::query(
                    "DELETE FROM temporal_entity_attributes tea USING attribute_names an WHERE an.id = tea.attribute_name_id AND tea.entity_id = $1 AND an.name = $2",
                )
                .bind(entity_id.as_str())
                .bind(attribute_name.as_str())
                .execute(&self.pool)
                .await?
            }
            DeleteScope::Single => match dataset_id {
                Some(dataset_id) => {
                    sqlx::query(
                        "DELETE FROM temporal_entity_attributes tea USING attribute_names an WHERE an.id = tea.attribute_name_id AND tea.entity_id = $1 AND an.name = $2 AND tea.dataset_id = $3",
                    )
                    .bind(entity_id.as_str())
                    .bind(attribute_name.as_str())
                    .bind(dataset_id.as_str())
                    .execute(&self.pool)
                    .await?
                }
                None => {
                    sqlx::query(
                        "DELETE FROM temporal_entity_attributes tea USING attribute_names an WHERE an.id = tea.attribute_name_id AND tea.entity_id = $1 AND an.name = $2 AND tea.dataset_id IS NULL",
                    )
                    .bind(entity_id.as_str())
                    .bind(attribute_name.as_str())
                    .execute(&self.pool)
                    .await?
                }
            },
        };
        Ok(result.rows_affected())
    }
}
