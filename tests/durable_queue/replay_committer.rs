//! The broker's own atomic committer over the suite's database, which is what
//! replays a leased side-effect job in these verifications.

use andromeda_context_commit::commit_consequence_publisher::NoCommitConsequencePublisher;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName};
use andromeda_postgresql_atomic_write::{
    committer::{EntityInsertMode, PostgresAtomicCommitter},
    history_mode::TemporalHistoryCommitMode,
};
use andromeda_postgresql_name_dictionary::{name_cache::NameCache, name_lookup::NameLookup};
use andromeda_postgresql_temporal_store::{
    bulk_write_mode::BulkWriteMode,
    columnstore_capability::ColumnstoreSupport,
    recorder::TemporalRecorder,
    repo::TemporalRepository,
};
use sqlx::PgPool;
use std::sync::Arc;

/// The broker's own committer over the suite's database. History is native;
/// the replays here record only notifications, which need no history.
pub fn committer(pool: &PgPool) -> PostgresAtomicCommitter {
    let attribute_names = Arc::new(NameCache::<AttributeName>::new(pool)) as Arc<dyn NameLookup<AttributeName>>;
    let entity_type_names = Arc::new(NameCache::<EntityTypeName>::new(pool)) as Arc<dyn NameLookup<EntityTypeName>>;
    let repository = Arc::new(TemporalRepository::new(
        pool,
        BulkWriteMode::default(),
        Arc::clone(&attribute_names),   // clone: the repository and the committer share one dictionary.
        Arc::clone(&entity_type_names), // clone: the same, for entity types.
        ColumnstoreSupport::Absent,
    ));
    PostgresAtomicCommitter::new(
        pool,
        TemporalHistoryCommitMode::Native {
            recorder: Arc::new(TemporalRecorder::new(repository)),
        },
        EntityInsertMode::Unnest,
        attribute_names,
        entity_type_names,
        Arc::new(NoCommitConsequencePublisher),
    )
}
