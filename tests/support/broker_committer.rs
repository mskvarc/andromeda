//! The broker's own atomic committer over a suite's database.
//!
//! A verification that must hand the committer writes no HTTP request can
//! produce -- a replay of a leased side-effect job, or content the admission
//! layer would have refused -- drives the committer the broker composes, over
//! the same tenant database the broker writes.

use andromeda_context_commit::commit_consequence_publisher::NoCommitConsequencePublisher;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName};
use andromeda_postgresql_atomic_write::{committer::PostgresAtomicCommitter, history_mode::TemporalHistoryCommitMode};
use andromeda_postgresql_name_dictionary::{name_cache::NameCache, name_cache_config::NameCacheConfig, name_lookup::NameLookup};
use andromeda_postgresql_temporal_store::{columnstore_capability::ColumnstoreSupport, recorder::TemporalRecorder, repo::TemporalRepository};
use andromeda_temporal::core_write_history::CoreWriteHistory;
use sqlx::PgPool;
use std::sync::Arc;

/// The broker's own committer over `pool`, recording history natively and every
/// Core write in it.
pub fn committer(pool: &PgPool) -> PostgresAtomicCommitter {
    let attribute_names = Arc::new(NameCache::<AttributeName>::new(pool, &NameCacheConfig::default())) as Arc<dyn NameLookup<AttributeName>>;
    let entity_type_names = Arc::new(NameCache::<EntityTypeName>::new(pool, &NameCacheConfig::default())) as Arc<dyn NameLookup<EntityTypeName>>;
    let repository = Arc::new(TemporalRepository::new(
        pool,
        Arc::clone(&attribute_names),   // clone: the repository and the committer share one dictionary.
        Arc::clone(&entity_type_names), // clone: the same, for entity types.
        ColumnstoreSupport::Absent,
    ));
    PostgresAtomicCommitter::new(
        pool,
        TemporalHistoryCommitMode::Native {
            recorder: Arc::new(TemporalRecorder::new(repository)),
        },
        CoreWriteHistory::Recorded,
        attribute_names,
        entity_type_names,
        Arc::new(NoCommitConsequencePublisher),
    )
}
