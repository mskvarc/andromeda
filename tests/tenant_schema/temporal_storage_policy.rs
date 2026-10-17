//! The physical storage policy of the temporal hypertable.
//!
//! The migrations declare seven-day chunks and nothing about the columnstore.
//! The broker's storage-policy reconciler enables the columnstore when a tenant
//! is opened, segmented by stream and ordered by the write-deduplication key,
//! and installs no `TimescaleDB` conversion job: the broker converts chunks
//! itself. The chunk interval is an operator tunable the broker reconciles on
//! the same open, so a fresh tenant must carry the default, an override must
//! move the hypertable, a second reconciliation of the same override must write
//! nothing, and a hypertable that lost its columnstore must regain it on the
//! next open. These run against the TSL edition of the image, which is the one
//! with a columnstore.
//!
//! Each test works on a private tenant the broker opened once: the broker
//! restates the configured policy every time it opens a tenant, and the
//! default tenant is reopened whenever the tier's tenant churn evicts it, which
//! overwrote an override between the test writing it and reading it back.

use crate::SUITE;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName};
use andromeda_postgresql_name_dictionary::{name_cache::NameCache, name_cache_config::NameCacheConfig};
use andromeda_postgresql_pool::{read_share::ReadShare, write_reserve::WriteReserve};
use andromeda_postgresql_temporal_store::{
    columnstore_capability::ColumnstoreSupport,
    repo::TemporalRepository,
    schema_ddl::timescale_policy::HypertableName,
    storage_policy::PolicyReconciliation,
};
use andromeda_temporal::storage_policy::TemporalStoragePolicy;
use andromeda_test_fixtures::context::CORE_CONTEXT;
use andromeda_test_harness::tenant_scoped_environment::TenantScopedEnvironment;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::json;
use serial_test::serial;
use sqlx::PgPool;
use std::{num::NonZeroU32, sync::Arc};

/// The temporal hypertables and the column each segments its columnstore by;
/// `scope_history` is a plain table.
const HYPERTABLES: &[(&str, &str)] = &[("attribute_history", "stream_key")];

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

/// How many `TimescaleDB` columnstore jobs act on `hypertable`.
async fn columnstore_jobs(pool: &PgPool, hypertable: &str) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM timescaledb_information.jobs WHERE hypertable_name = $1 AND proc_name = 'policy_compression'")
        .bind(hypertable)
        .fetch_one(pool)
        .await
        .expect("the jobs view must be readable")
}

/// The database of a private tenant the broker has opened, and so reconciled,
/// exactly once: the test sends it no further request, so no reopening
/// restates the configured policy over what the test writes.
async fn opened_tenant_pool(scope: &TenantScopedEnvironment) -> PgPool {
    let created = scope
        .client()
        .post_entity(&json!({"id": scope.id("Sensor:storage-policy"), "type": "Sensor", "@context": CORE_CONTEXT}))
        .send()
        .await
        .expect("create entity request");
    assert_eq!(
        created.status,
        StatusCode::CREATED,
        "the write brings the tenant into being: {:?}",
        created.body
    );
    scope.tenant_database().await.expect("the tenant database exists once an entity was written")
}

/// A repository over the suite's tenant, with throwaway dictionary caches.
async fn repository(pool: &PgPool) -> TemporalRepository {
    let columnstore = ColumnstoreSupport::detect(pool).await.expect("the columnstore capability probe runs");
    TemporalRepository::new(
        pool,
        ReadShare::of(pool, WriteReserve::DEFAULT),
        Arc::new(NameCache::<AttributeName>::new(pool, &NameCacheConfig::default())),
        Arc::new(NameCache::<EntityTypeName>::new(pool, &NameCacheConfig::default())),
        columnstore,
    )
}

/// The policy naming `chunk_interval_days` and `columnstore_after_days`.
const fn policy(chunk_interval_days: u32, columnstore_after_days: u32) -> TemporalStoragePolicy {
    TemporalStoragePolicy {
        chunk_interval_days: NonZeroU32::new(chunk_interval_days),
        columnstore_after_days: NonZeroU32::new(columnstore_after_days).expect("a positive columnstore lag"),
    }
}

/// Whether the columnstore is enabled on `hypertable`.
async fn columnstore_enabled(pool: &PgPool, hypertable: &str) -> bool {
    sqlx::query_scalar("SELECT compression_enabled FROM timescaledb_information.hypertables WHERE hypertable_name = $1")
        .bind(hypertable)
        .fetch_one(pool)
        .await
        .expect("the hypertables view must list the hypertable")
}

/// A fresh tenant carries the migration's seven-day chunks, and the columnstore
/// the reconciler enabled when the broker opened it: the hypertable segments
/// and order rows the way `columnstore_enablement` states, with no conversion
/// job installed.
#[tokio::test]
#[serial(temporal_storage_policy)]
async fn tenant_schema_the_columnstore_is_enabled_by_the_reconciler_on_a_fresh_tenant() {
    let scope = TenantScopedEnvironment::from_suite(SUITE).await.expect("private tenant environment");
    let tenant_pool = opened_tenant_pool(&scope).await;
    let pool = &tenant_pool;

    // The broker that opened this tenant reconciled against a configuration
    // without overrides, so whatever is stored is the default policy;
    // restating the defaults through the reconciler must therefore write
    // nothing and leaves the assertions below meaningful even after another
    // test of this suite has moved the policy and moved it back.
    let outcome = repository(pool)
        .await
        .reconcile_storage_policy(&policy(7, 30))
        .await
        .expect("the reconciliation runs");
    assert!(outcome.is_unchanged(), "a fresh tenant already carries the defaults: {outcome:?}");

    for (hypertable, segmentby) in HYPERTABLES {
        assert!(columnstore_enabled(pool, hypertable).await, "{hypertable} carries the columnstore");
        assert_days(
            chunk_interval_days(pool, hypertable).await,
            7.0,
            &format!("{hypertable} chunks span seven days"),
        );
        assert_eq!(columnstore_jobs(pool, hypertable).await, 0, "{hypertable} depends on no conversion job");

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

    tenant_pool.close().await;
    scope.finish().await.expect("cleanup failed");
}

/// An operator override moves the hypertable's chunk interval, a second
/// reconciliation of the same override is a read and no write, and
/// restating the defaults moves it back.
#[tokio::test]
#[serial(temporal_storage_policy)]
async fn an_override_moves_the_hypertable_once_and_the_defaults_move_it_back() {
    let scope = TenantScopedEnvironment::from_suite(SUITE).await.expect("private tenant environment");
    let tenant_pool = opened_tenant_pool(&scope).await;
    let pool = &tenant_pool;
    let repository = repository(pool).await;

    let overridden = repository.reconcile_storage_policy(&policy(1, 3)).await.expect("the override reconciles");
    assert_eq!(overridden.hypertables.len(), 1, "the hypertable is reconciled");
    for reconciliation in &overridden.hypertables {
        assert_eq!(reconciliation.chunk_interval, PolicyReconciliation::Adjusted, "{:?}", reconciliation.hypertable);
    }
    assert_eq!(
        overridden
            .hypertables
            .iter()
            .map(|reconciliation| reconciliation.hypertable)
            .collect::<Vec<_>>(),
        vec![HypertableName::AttributeHistory]
    );
    for (hypertable, _) in HYPERTABLES {
        assert_days(
            chunk_interval_days(pool, hypertable).await,
            1.0,
            &format!("{hypertable} chunks now span one day"),
        );
        assert_eq!(columnstore_jobs(pool, hypertable).await, 0, "{hypertable} depends on no conversion job");
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
        assert_eq!(columnstore_jobs(pool, hypertable).await, 0, "{hypertable} depends on no conversion job");
    }

    tenant_pool.close().await;
    scope.finish().await.expect("cleanup failed");
}

/// A tenant whose hypertables lost the columnstore -- one migrated on the
/// Apache-2.0 core and now opened on the TSL edition looks exactly like this --
/// regains it, with its settings, the next time the broker opens it.
#[tokio::test]
#[serial(temporal_storage_policy)]
async fn tenant_schema_a_hypertable_without_the_columnstore_is_enabled_on_the_next_open() {
    let scope = TenantScopedEnvironment::from_suite(SUITE).await.expect("private tenant environment");
    let tenant_pool = opened_tenant_pool(&scope).await;
    let pool = &tenant_pool;

    for (hypertable, _) in HYPERTABLES {
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "ALTER TABLE {hypertable} SET (timescaledb.enable_columnstore = false)"
        )))
        .execute(pool)
        .await
        .expect("the columnstore is disabled");
        assert!(
            !columnstore_enabled(pool, hypertable).await,
            "{hypertable} is a plain hypertable before the open"
        );
    }

    let reopened = repository(pool)
        .await
        .reconcile_storage_policy(&policy(7, 30))
        .await
        .expect("the reconciliation runs");
    for reconciliation in &reopened.hypertables {
        assert_eq!(
            reconciliation.columnstore_enabled,
            PolicyReconciliation::Adjusted,
            "{:?}",
            reconciliation.hypertable
        );
    }
    for (hypertable, _) in HYPERTABLES {
        assert!(columnstore_enabled(pool, hypertable).await, "{hypertable} carries the columnstore again");
        assert_eq!(columnstore_jobs(pool, hypertable).await, 0, "{hypertable} depends on no conversion job");
    }

    let settled = repository(pool)
        .await
        .reconcile_storage_policy(&policy(7, 30))
        .await
        .expect("the reconciliation runs");
    assert!(settled.is_unchanged(), "a second open writes nothing: {settled:?}");

    tenant_pool.close().await;
    scope.finish().await.expect("cleanup failed");
}
