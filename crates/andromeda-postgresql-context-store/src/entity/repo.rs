//! The PostgreSQL entity store.
//!
//! Holds the tenant pool plus the two dictionaries every name is stored as a
//! key of: the attribute-name dictionary the `current_attributes` rows are
//! keyed on and the entity-type-name dictionary the `entity_type_name_keys`
//! column and the rows' type keys are keyed on. A pushed-down predicate
//! resolves names to keys through them and a read resolves the keys it reads
//! back to names. Those are properties of this store, not of the policy above
//! it, which is why they live here and not in the port.
//!
//! An Entity's rows are written inside the same transaction as its Entity row,
//! so a committed write is a visible write and a reader that can see the
//! Entity can see its attributes.

use crate::error::ContextStoreError;
use andromeda_entity::storage_profile::StorageProfile;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName};
use andromeda_ngsi_ld_query::ordering::compiled::ResultOrdering;
use andromeda_postgresql_collation::provisioned_collations::ProvisionedCollations;
use andromeda_postgresql_name_dictionary::name_lookup::NameLookup;
use sqlx::postgres::PgPool;
use std::sync::Arc;

/// Repository for persisting and querying NGSI-LD entities.
#[derive(Debug)]
pub struct EntityRepository {
    /// The tenant's connection pool.
    pub(super) pool: PgPool,
    /// The attribute-name dictionary, shared with the temporal store.
    pub(super) attribute_names: Arc<dyn NameLookup<AttributeName>>,
    /// The entity-type-name dictionary, shared with the temporal store.
    pub(super) entity_type_names: Arc<dyn NameLookup<EntityTypeName>>,
    /// ICU collations already provisioned on this tenant's database.
    pub(super) collations: ProvisionedCollations,
}

impl EntityRepository {
    /// What this store can be asked to do with a compiled query.
    ///
    /// A constant rather than a probe: the tenant migrations create the `PostGIS` geometry columns and the `ltree` scope columns, so a pool that migrated has them and one that could not migrate never becomes a store. That is the difference between these axes and the `TimescaleDB` edition, which is genuinely per-pool runtime state.
    pub const PROFILE: StorageProfile = StorageProfile::POSTGIS_LTREE;

    /// Create a new entity repository backed by the given connection pool.
    pub fn new(pool: &PgPool, attribute_names: Arc<dyn NameLookup<AttributeName>>, entity_type_names: Arc<dyn NameLookup<EntityTypeName>>) -> EntityRepository {
        EntityRepository {
            pool: pool.clone(), // clone: Arc ref-count bump
            attribute_names,
            entity_type_names,
            collations: ProvisionedCollations::of_pool(pool),
        }
    }

    /// Provision every ICU collation a clause-4.23 ordering names.
    ///
    /// # Errors
    ///
    /// Returns [`ContextStoreError::Collation`] when a collation cannot be
    /// created on the tenant's database.
    pub(super) async fn ensure_collations(&self, ordering: &ResultOrdering) -> Result<(), ContextStoreError> {
        for collation in ordering.terms.iter().filter_map(|term| term.collation.as_ref()) {
            self.collations.ensure(collation).await?;
        }
        Ok(())
    }
}
