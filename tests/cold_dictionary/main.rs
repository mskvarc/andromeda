//! Cold attribute-name dictionary integration tests.
//!
//! Two properties the memory note conflated are pinned down here. The first is
//! that the *live* cold-mint path is race-free: many transactions first
//! populating the dictionary with an overlapping set of never-seen attribute
//! names all commit, and every committed `temporal_attribute_streams` row
//! references a committed `attribute_names` row -- no
//! `temporal_attribute_streams_attribute_name_id_fkey`. The second is that the
//! foreign-key 500 seen in the benchmark was an operational artifact: a stale
//! in-memory `NameCache` surviving an out-of-band tenant schema drop and
//! recreate, which the schema-epoch check now detects and repairs.
//!
//! Naming convention: `cold_dictionary_<scenario>_<expected_outcome>`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_client::{broker_endpoint::BrokerEndpoint, tenant_name::TenantName, test_client::TestClient};
use andromeda_test_fixtures::context::CORE_CONTEXT;
use andromeda_test_harness::{
    configuration::{DATABASE_ENDPOINT_ENV, DEFAULT_DATABASE_ENDPOINT},
    environment::TestEnvironment,
    tenant_schema_recreate::simulate_out_of_band_recreate,
};
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::{Map, Value, json};
use sqlx::postgres::PgPool;
use std::{env, slice, time::Duration};
use tokio::time::sleep;
use uuid::Uuid;

/// A fresh attribute IRI the dictionary has never held, so resolving it is a
/// genuine cold mint.
fn cold_attribute() -> String {
    format!("https://cold.example/attr/{}", Uuid::new_v4())
}

/// A `Widget` entity carrying one Property per attribute IRI.
///
/// The keys are absolute IRIs, so each is its own expanded attribute name and
/// reaches the dictionary exactly as written.
fn widget(id: &str, attributes: &[String]) -> Value {
    let mut object = Map::new();
    object.insert("id".to_owned(), json!(id));
    object.insert("type".to_owned(), json!("Widget"));
    for (ordinal, attribute) in attributes.iter().enumerate() {
        let ordinal = i64::try_from(ordinal).expect("attribute ordinal fits in i64");
        object.insert(attribute.clone(), json!({ "type": "Property", "value": ordinal }));
    }
    object.insert("@context".to_owned(), json!([CORE_CONTEXT]));
    Value::Object(object)
}

/// The base tenant database URL, honoring the harness environment override.
fn database_base() -> String {
    env::var(DATABASE_ENDPOINT_ENV).unwrap_or_else(|_| DEFAULT_DATABASE_ENDPOINT.to_owned())
}

/// The same server, a different database: the last path segment is the database
/// name, so swapping it re-points the URL at one tenant's database.
fn with_database(base: &str, database: &str) -> String {
    match base.rfind('/') {
        Some(boundary) => format!("{}/{database}", &base[..boundary]),
        None => format!("{base}/{database}"),
    }
}

/// N concurrent transactions each first minting an overlapping set of cold
/// attribute names all commit, and every committed stream references a
/// committed dictionary row. The foreign-key 500 the memory note feared would
/// surface here as a non-created status or an orphaned stream.
#[tokio::test]
async fn cold_dictionary_concurrent_cold_mints_commit_without_a_foreign_key_violation() {
    const WORKERS: usize = 8;
    let scope = TestEnvironment::from_suite("cold_dictionary").await.expect("isolated test environment");
    let cold: Vec<String> = (0..5).map(|_| cold_attribute()).collect();

    let mut workers = Vec::with_capacity(WORKERS);
    for worker in 0..WORKERS {
        let client = scope.client().clone(); // clone: each spawned worker owns a handle to the shared broker endpoint
        let id = scope.id(&format!("Widget:cold-{worker}"));
        let cold = cold.clone(); // clone: each worker owns the shared cold-name set it mints
        workers.push(tokio::spawn(async move {
            let entity = widget(&id, &cold);
            client.post_entity(&entity).send().await.map(|response| response.status)
        }));
    }

    let mut statuses = Vec::with_capacity(WORKERS);
    for worker in workers {
        statuses.push(worker.await.expect("worker task joins").expect("request sent"));
    }
    for status in &statuses {
        assert_eq!(*status, StatusCode::CREATED, "a concurrent cold mint failed with {status}");
    }

    let minted: Vec<String> = sqlx::query_scalar("SELECT name FROM attribute_names WHERE name = ANY($1)")
        .bind(&cold)
        .fetch_all(scope.database_pool())
        .await
        .expect("dictionary query");
    assert_eq!(minted.len(), cold.len(), "every cold name is minted exactly once despite the concurrent race");

    let orphans: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM temporal_attribute_streams s \
         LEFT JOIN attribute_names n ON s.attribute_name_id = n.id \
         WHERE n.id IS NULL",
    )
    .fetch_one(scope.database_pool())
    .await
    .expect("orphan query");
    assert_eq!(orphans, 0, "no committed stream references a missing attribute name");

    scope.finish().await.expect("cleanup");
}

/// One transaction mints a cold name and then rolls back on an entity-id
/// conflict while a sibling mints the same name for a fresh entity and commits.
/// The sibling's own insert must resolve the name to a valid committed id, and
/// the name must end up minted exactly once.
#[tokio::test]
async fn cold_dictionary_a_rolled_back_mint_leaves_its_sibling_committing_a_valid_id() {
    let scope = TestEnvironment::from_suite("cold_dictionary").await.expect("isolated test environment");
    let cold = cold_attribute();
    let existing = scope.id("Widget:abort-existing");
    let fresh = scope.id("Widget:abort-fresh");

    let seed = widget(&existing, &[cold_attribute()]);
    assert_eq!(
        scope.client().post_entity(&seed).send().await.expect("seed write").status,
        StatusCode::CREATED,
        "the seed entity is created"
    );

    let aborting = {
        let client = scope.client().clone(); // clone: the aborting worker owns its client handle
        let entity = widget(&existing, slice::from_ref(&cold));
        tokio::spawn(async move { client.post_entity(&entity).send().await.map(|response| response.status) })
    };
    let committing = {
        let client = scope.client().clone(); // clone: the committing worker owns its client handle
        let entity = widget(&fresh, slice::from_ref(&cold));
        tokio::spawn(async move { client.post_entity(&entity).send().await.map(|response| response.status) })
    };

    let aborted = aborting.await.expect("aborting task joins").expect("request sent");
    let committed = committing.await.expect("committing task joins").expect("request sent");
    assert_eq!(aborted, StatusCode::CONFLICT, "re-creating an existing id conflicts and rolls back");
    assert_eq!(committed, StatusCode::CREATED, "the sibling commits its own mint of the contended name");

    let minted: Vec<String> = sqlx::query_scalar("SELECT name FROM attribute_names WHERE name = $1")
        .bind(&cold)
        .fetch_all(scope.database_pool())
        .await
        .expect("dictionary query");
    assert_eq!(minted.len(), 1, "the contended name is minted exactly once");

    scope.finish().await.expect("cleanup");
}

/// A tenant database dropped and recreated under the same name -- reset here to
/// a fresh dictionary generation with an advanced schema stamp, the exact
/// signature of an out-of-band re-migrate -- must not leave the in-memory
/// `NameCache` serving keys of the previous generation. The next write detects
/// the changed stamp, discards the cache, re-mints the name, and commits rather
/// than raising the attribute-name foreign key.
#[tokio::test]
async fn cold_dictionary_a_recreated_tenant_schema_refreshes_the_name_cache() {
    let endpoint = BrokerEndpoint::from_environment().expect("broker endpoint");
    let tenant_raw = format!("coldcache{}", Uuid::new_v4().simple());
    let tenant = TenantName::parse(&tenant_raw).expect("valid tenant name");
    let client = TestClient::new(endpoint, Some(tenant));
    let cold = cold_attribute();

    // Two writes: the first mints the cold name (held back from the cache until
    // committed), the second reads it from a committed snapshot and publishes it
    // to the cache -- so the cache genuinely holds the mapping before the reset.
    for ordinal in 1..=2 {
        let entity = widget(&format!("urn:ngsi-ld:Widget:warm-{ordinal}"), slice::from_ref(&cold));
        assert_eq!(
            client.post_entity(&entity).send().await.expect("warm write").status,
            StatusCode::CREATED,
            "the warm-up write commits"
        );
    }

    let admin = PgPool::connect(&database_base()).await.expect("admin pool");
    let database: String = sqlx::query_scalar("SELECT db_name FROM tenants WHERE name = $1")
        .bind(&tenant_raw)
        .fetch_one(&admin)
        .await
        .expect("tenant registry row");
    let tenant_pool = PgPool::connect(&with_database(&database_base(), &database)).await.expect("tenant pool");

    // Out-of-band surgery: reset the dictionary so its `IDENTITY` restarts and
    // advance the schema stamp, the signature the epoch check reads as a
    // recreate. The global reset SQL lives in the harness, not inline here.
    simulate_out_of_band_recreate(&tenant_pool).await.expect("simulate schema recreate");

    // Wait out the epoch-check throttle so the next resolution re-reads the stamp.
    sleep(Duration::from_secs(6)).await;

    let entity = widget("urn:ngsi-ld:Widget:after-recreate", slice::from_ref(&cold));
    assert_eq!(
        client.post_entity(&entity).send().await.expect("post-recreate write").status,
        StatusCode::CREATED,
        "the recreated schema is detected and the stale cache refreshed"
    );

    let present: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM attribute_names WHERE name = $1")
        .bind(&cold)
        .fetch_one(&tenant_pool)
        .await
        .expect("dictionary query");
    assert_eq!(present, 1, "the name is re-minted once in the fresh dictionary");

    admin.close().await;
    tenant_pool.close().await;
}
