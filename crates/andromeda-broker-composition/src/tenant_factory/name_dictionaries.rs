//! The tenant's attribute and entity type name dictionary caches, opened and
//! warmed as the tenant's storage comes up.

use crate::tenant_factory::TenantFactoryConfig;
use andromeda_configuration::schema::database::name_cache::NameLoading;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName};
use andromeda_postgresql_name_dictionary::{name_cache::NameCache, name_lookup::NameLookup, schema_epoch::read_established_epoch};
use sqlx::PgPool;
use std::sync::Arc;

/// The tenant's attribute and entity type name dictionary caches, warmed as
/// the configuration asks.
///
/// One cache per dictionary per tenant, shared by every path that resolves an
/// attribute or type IRI: the temporal catalogue, the entity repository's read
/// path and the entity rows and projection the commit writes.
pub(super) async fn name_dictionaries(
    pool: &PgPool,
    config: &TenantFactoryConfig,
) -> (Arc<dyn NameLookup<AttributeName>>, Arc<dyn NameLookup<EntityTypeName>>) {
    let attribute_names = Arc::new(NameCache::<AttributeName>::new(pool, &config.database.name_cache));
    let entity_type_names = Arc::new(NameCache::<EntityTypeName>::new(pool, &config.database.name_cache));
    // Read the schema stamp once and hand it to both dictionaries as their
    // baseline, so a later out-of-band drop and recreate of this tenant is
    // detected as a change from a known baseline rather than adopted silently,
    // and the two caches share one catalogue read instead of each issuing its own.
    match read_established_epoch(pool).await {
        Ok(epoch) => {
            attribute_names.adopt_epoch(epoch);
            entity_type_names.adopt_epoch(epoch);
        }
        Err(error) => tracing::warn!(%error, event = "name_dictionary_epoch_unestablished"),
    }
    if matches!(config.database.name_cache.loading, NameLoading::Eager) {
        if let Err(error) = attribute_names.preload().await {
            tracing::warn!(%error, event = "attribute_names_preload_failed");
        }
        if let Err(error) = entity_type_names.preload().await {
            tracing::warn!(%error, event = "entity_type_names_preload_failed");
        }
    }
    (
        attribute_names as Arc<dyn NameLookup<AttributeName>>,
        entity_type_names as Arc<dyn NameLookup<EntityTypeName>>,
    )
}
