//! The physical storage policy of the temporal hypertables.
//!
//! Migrations 0011 and 0012 declare seven-day chunks, a columnstore segmented
//! by stream (attribute history) or temporal entity (scope history) and ordered
//! by the write-deduplication key, and a conversion policy that lags thirty
//! days. Both intervals are operator tunables the broker reconciles when a
//! tenant is opened, so a fresh tenant must carry the declared defaults, an
//! override must move both hypertables, and a second reconciliation of the same
//! override must write nothing.

use crate::SUITE;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName};
use andromeda_postgresql_name_dictionary::name_cache::NameCache;
use andromeda_postgresql_temporal_store::{
    bulk_write_mode::BulkWriteMode,
    repo::TemporalRepository,
    schema_ddl::timescale_policy::HypertableName,
    storage_policy::PolicyReconciliation,
};
use andromeda_temporal::storage_policy::TemporalStoragePolicy;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use serial_test::serial;
use sqlx::PgPool;
use std::{num::NonZeroU32, sync::Arc};

/// The two hypertables and the column each segments its columnstore by.
const HYPERTABLES: &[(&str, &str)] = &[
    ("attribute_history", "context_space_key,stream_key"),
    ("scope_history", "context_space_key,temporal_entity_key"),
];

/// Assert a policy day count, which Timescale reports as a float built from an
/// interval rather than as the integer the policy declared.
fn assert_days(actual: f64, expected: f64, context: &str) {
    assert!((actual - expected).abs() < f64::EPSILON, "{context}: {actual}");
}

/// The chunk interval of `hypertable`, in whole days.
async fn chunk_interval_days(pool: &PgPool, hypertable: &str) -> f64 {
    sqlx::query_scalar(
        "SELECT (EXTRACT(EPOCH FROM time_interval) / 86400)::float8 FROM timescaledb_information.dimensions \
         WHERE hypertable_name = $1 AND dimension_number = 1",
    )
    .bind(hypertable)
    .fetch_one(pool)
    .await
    .expect("the dimension view must be readable")
}

/// The columnstore policy lag of `hypertable`, in whole days.
async fn columnstore_after_days(pool: &PgPool, hypertable: &str) -> f64 {
    sqlx::query_scalar(
        "SELECT (EXTRACT(EPOCH FROM (config ->> 'compress_after')::interval) / 86400)::float8 FROM timescaledb_information.jobs \
         WHERE hypertable_name = $1 AND proc_name = 'policy_compression'",
    )
    .bind(hypertable)
    .fetch_one(pool)
    .await
    .expect("the policy job must exist and be readable")
}

/// A repository over the suite's tenant, with throwaway dictionary caches.
fn repository(pool: &PgPool) -> TemporalRepository {
    TemporalRepository::new(
        pool,
        BulkWriteMode::Unnest,
        Arc::new(NameCache::<AttributeName>::new(pool)),
        Arc::new(NameCache::<EntityTypeName>::new(pool)),
    )
}

/// The policy `days` names for both values.
const fn policy(chunk_interval_days: u32, columnstore_after_days: u32) -> TemporalStoragePolicy {
    TemporalStoragePolicy {
        chunk_interval_days: NonZeroU32::new(chunk_interval_days),
        columnstore_after_days: NonZeroU32::new(columnstore_after_days),
    }
}

/// A fresh tenant carries the migrations' seven-day chunks and thirty-day
/// conversion lag on both hypertables, and its columnstore settings segment
/// and order rows the way the migration headers state.
#[tokio::test]
#[serial(temporal_storage_policy)]
async fn a_fresh_tenant_carries_the_declared_chunk_interval_columnstore_settings_and_policy_lag() {
    let scope = TestEnvironment::from_suite(SUITE).await.expect("isolated test environment");
    let pool = scope.database_pool();

    // The broker that opened this tenant reconciled against a configuration
    // without overrides, so whatever is stored is what the migrations declared;
    // restating the defaults through the reconciler must therefore write
    // nothing and leaves the assertions below meaningful even after another
    // test of this suite has moved the policy and moved it back.
    let outcome = repository(pool)
        .reconcile_storage_policy(&policy(7, 30))
        .await
        .expect("the reconciliation runs");
    assert!(outcome.is_unchanged(), "a fresh tenant already carries the defaults: {outcome:?}");

    for (hypertable, segmentby) in HYPERTABLES {
        assert_days(
            chunk_interval_days(pool, hypertable).await,
            7.0,
            &format!("{hypertable} chunks span seven days"),
        );
        assert_days(
            columnstore_after_days(pool, hypertable).await,
            30.0,
            &format!("{hypertable} converts after thirty days"),
        );

        let (stored_segmentby, stored_orderby): (String, String) =
            sqlx::query_as("SELECT segmentby, orderby FROM timescaledb_information.hypertable_columnstore_settings WHERE hypertable = $1::regclass")
                .bind(hypertable)
                .fetch_one(pool)
                .await
                .expect("the columnstore settings view must list the hypertable");
        assert_eq!(stored_segmentby, *segmentby, "{hypertable} segments by its grain");
        // The settings view renders the order-by list without spaces.
        assert_eq!(
            stored_orderby, "recorded_at DESC,write_id,write_ordinal",
            "{hypertable} orders each segment by the write-deduplication key"
        );
    }

    scope.finish().await.expect("cleanup failed");
}

/// An operator override moves both hypertables' chunk interval and policy lag,
/// a second reconciliation of the same override is a read and no write, and
/// restating the defaults moves both back.
#[tokio::test]
#[serial(temporal_storage_policy)]
async fn an_override_moves_both_hypertables_once_and_the_defaults_move_them_back() {
    let scope = TestEnvironment::from_suite(SUITE).await.expect("isolated test environment");
    let pool = scope.database_pool();
    let repository = repository(pool);

    let overridden = repository.reconcile_storage_policy(&policy(1, 3)).await.expect("the override reconciles");
    assert_eq!(overridden.hypertables.len(), 2, "both hypertables are reconciled");
    for reconciliation in &overridden.hypertables {
        assert_eq!(reconciliation.chunk_interval, PolicyReconciliation::Adjusted, "{:?}", reconciliation.hypertable);
        assert_eq!(
            reconciliation.columnstore_after,
            PolicyReconciliation::Adjusted,
            "{:?}",
            reconciliation.hypertable
        );
    }
    assert_eq!(
        overridden
            .hypertables
            .iter()
            .map(|reconciliation| reconciliation.hypertable)
            .collect::<Vec<_>>(),
        vec![HypertableName::AttributeHistory, HypertableName::ScopeHistory]
    );
    for (hypertable, _) in HYPERTABLES {
        assert_days(
            chunk_interval_days(pool, hypertable).await,
            1.0,
            &format!("{hypertable} chunks now span one day"),
        );
        assert_days(
            columnstore_after_days(pool, hypertable).await,
            3.0,
            &format!("{hypertable} now converts after three days"),
        );
    }

    let restated = repository.reconcile_storage_policy(&policy(1, 3)).await.expect("the restatement reconciles");
    assert!(restated.is_unchanged(), "restating the override writes nothing: {restated:?}");

    let restored = repository.reconcile_storage_policy(&policy(7, 30)).await.expect("the defaults reconcile");
    assert!(!restored.is_unchanged(), "restoring the defaults is a write: {restored:?}");
    for (hypertable, _) in HYPERTABLES {
        assert_days(
            chunk_interval_days(pool, hypertable).await,
            7.0,
            &format!("{hypertable} chunks span seven days again"),
        );
        assert_days(
            columnstore_after_days(pool, hypertable).await,
            30.0,
            &format!("{hypertable} converts after thirty days again"),
        );
    }

    scope.finish().await.expect("cleanup failed");
}
