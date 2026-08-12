//! Removing projection rows an entity mutation no longer stands behind.

use crate::{current_attribute_values::row::ProjectedAttributeKey, error::ContextStoreError};
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_postgresql_codes::entity_key::EntityKey;
use andromeda_postgresql_query::attribute_name_id::AttributeNameId;
use sqlx::{Postgres, Transaction};

/// Delete the projection rows of the named attribute instances.
///
/// Deleting an attribute instance (clause 5.6.13) or an attribute in full
/// (clause 5.6.12, which addresses every instance of it) removes the row rather
/// than marking it: the projection answers what an entity holds now, and an
/// instance that is gone holds nothing. The temporal evolution keeps its own
/// record of the deletion and is untouched by this.
///
/// Runs in the caller's transaction, so the removal and the entity mutation
/// that ordered it commit together.
///
/// # Errors
///
/// Returns [`ContextStoreError::Database`] when the statement fails.
pub async fn delete_instances_in_tx(tx: &mut Transaction<'_, Postgres>, keys: &[ProjectedAttributeKey]) -> Result<(), ContextStoreError> {
    if keys.is_empty() {
        return Ok(());
    }

    let mut entity_keys = Vec::with_capacity(keys.len());
    let mut attribute_name_ids = Vec::with_capacity(keys.len());
    let mut dataset_ids = Vec::with_capacity(keys.len());
    for key in keys {
        entity_keys.push(EntityKey::get(key.entity_key));
        attribute_name_ids.push(AttributeNameId::get(key.attribute_name_id));
        dataset_ids.push(key.dataset_id.as_ref().map(Uri::as_str));
    }
    let (entity_keys, attribute_name_ids, dataset_ids) = (entity_keys, attribute_name_ids, dataset_ids);

    sqlx::query(DELETE_INSTANCES_SQL)
        .bind(&entity_keys)
        .bind(&attribute_name_ids)
        .bind(&dataset_ids)
        .execute(&mut **tx)
        .await?;

    Ok(())
}

/// Delete every projection row of the named entities.
///
/// Only for a mutation that empties an entity's attributes without removing the
/// entity itself. Deleting the entity needs no call at all: the projection's
/// single foreign key cascades from `entities`, and that one ownership edge is
/// the whole deletion path.
///
/// # Errors
///
/// Returns [`ContextStoreError::Database`] when the statement fails.
pub async fn delete_entity_projections_in_tx(tx: &mut Transaction<'_, Postgres>, entity_keys: &[EntityKey]) -> Result<(), ContextStoreError> {
    if entity_keys.is_empty() {
        return Ok(());
    }

    let keys: Vec<i64> = entity_keys.iter().copied().map(EntityKey::get).collect();
    sqlx::query(DELETE_ENTITY_PROJECTIONS_SQL).bind(&keys).execute(&mut **tx).await?;

    Ok(())
}

/// Delete statement for a set of attribute instances.
///
/// The `datasetId` comparison is `IS NOT DISTINCT FROM` rather than `=`,
/// because the default instance is the one carrying no `datasetId` at all
/// (clause 4.5.5) and an equality test against a null matches nothing — the
/// default instance would survive every delete aimed at it.
const DELETE_INSTANCES_SQL: &str = r#"
DELETE FROM current_attribute_values c
USING UNNEST($1::bigint[], $2::integer[], $3::text[]) AS d(entity_key, attribute_name_id, dataset_id)
WHERE c.entity_key = d.entity_key
  AND c.attribute_name_id = d.attribute_name_id
  AND c.dataset_id IS NOT DISTINCT FROM d.dataset_id
"#;

/// Delete statement for every projection row of a set of entities.
const DELETE_ENTITY_PROJECTIONS_SQL: &str = "DELETE FROM current_attribute_values WHERE entity_key = ANY($1::bigint[])";

#[cfg(test)]
mod tests {
    use crate::current_attribute_values::delete::{DELETE_ENTITY_PROJECTIONS_SQL, DELETE_INSTANCES_SQL};

    #[test]
    fn the_instance_delete_reaches_the_default_instance() {
        assert!(
            DELETE_INSTANCES_SQL.contains("c.dataset_id IS NOT DISTINCT FROM d.dataset_id"),
            "an equality test would never match the null datasetId of a default instance"
        );
    }

    #[test]
    fn the_instance_delete_is_confined_to_one_entity_and_one_attribute() {
        assert!(DELETE_INSTANCES_SQL.contains("c.entity_key = d.entity_key"));
        assert!(DELETE_INSTANCES_SQL.contains("c.attribute_name_id = d.attribute_name_id"));
    }

    #[test]
    fn the_entity_delete_addresses_the_surrogate_key_alone() {
        assert!(DELETE_ENTITY_PROJECTIONS_SQL.contains("WHERE entity_key = ANY($1::bigint[])"));
    }
}
