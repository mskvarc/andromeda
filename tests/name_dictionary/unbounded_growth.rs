//! Attribute dictionary growth: minting new attribute names is never refused.
//!
//! Attribute IRIs are open-world (clause 4.5.2), so a tenant whose data keeps
//! introducing new names is writing valid NGSI-LD however large its dictionary
//! has grown. The broker bounds only its in-process cache of the dictionary,
//! never the dictionary itself, so a write that brings a new name is accepted
//! at any dictionary size and a write of an existing name keeps resolving it.

use crate::{GENERATED_NAME_PREFIX, SUITE};
use andromeda_test_client::test_client::TestClient;
use andromeda_test_fixtures::context::CORE_CONTEXT;
use andromeda_test_harness::{configuration::TestRunConfiguration, database_access::connect_test_database, serialized_environment::SerializedEnvironment};
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

/// Names interned before the write under test.
///
/// Past the 10,000-entry cap the broker used to enforce per dictionary, so the
/// check fails if any such cap comes back.
const NAMES_INTERNED_FIRST: i32 = 11_000;

/// An entity carrying one attribute under the core context's default vocabulary.
fn entity_with_attribute(id: &str, term: &str) -> Value {
    json!({
        "id": id,
        "type": "Device",
        term: {"type": "Property", "value": 1},
        "@context": [CORE_CONTEXT]
    })
}

/// Remove exactly the names this check interned.
async fn remove_generated_names(pool: &PgPool) {
    sqlx::query("DELETE FROM attribute_names WHERE name LIKE $1")
        .bind(format!("{GENERATED_NAME_PREFIX}%"))
        .execute(pool)
        .await
        .expect("the generated dictionary entries must be removable");
}

/// Delete an entity, tolerating one that was never created.
async fn discard(client: &TestClient, id: &str) {
    let _discarded = client.delete_entity(id).await;
}

#[tokio::test]
async fn a_dictionary_past_ten_thousand_names_still_mints_new_names_and_serves_existing_ones() {
    let configuration = TestRunConfiguration::from_environment().expect("integration test configuration");
    let environment = SerializedEnvironment::acquire(&configuration)
        .await
        .expect("exclusive ownership of the shared dictionary");
    let client = environment.client();
    let pool = connect_test_database().await.expect("test database connection");
    let run = Uuid::new_v4();
    let established_term = format!("dictionaryEstablished{}", run.simple());
    let novel_term = format!("dictionaryNovel{}", run.simple());
    let established_entity = format!("urn:ngsi-ld:test:{SUITE}:Device:established:{run}");
    let novel_entity = format!("urn:ngsi-ld:test:{SUITE}:Device:novel:{run}");
    let repeat_entity = format!("urn:ngsi-ld:test:{SUITE}:Device:repeat:{run}");

    remove_generated_names(&pool).await;

    let established = client
        .post_entity(&entity_with_attribute(&established_entity, &established_term))
        .send()
        .await
        .expect("create entity request failed");
    assert_eq!(established.status, StatusCode::CREATED, "create: {:?}", established.body);

    sqlx::query("INSERT INTO attribute_names (name) SELECT $1 || generated::TEXT FROM generate_series(1, $2) AS generated ON CONFLICT (name) DO NOTHING")
        .bind(GENERATED_NAME_PREFIX)
        .bind(NAMES_INTERNED_FIRST)
        .execute(&pool)
        .await
        .expect("interning distinct attribute names must not fail");
    let size: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM attribute_names")
        .fetch_one(&pool)
        .await
        .expect("the dictionary size must be observable");

    let novel = client
        .post_entity(&entity_with_attribute(&novel_entity, &novel_term))
        .send()
        .await
        .expect("create entity request failed");
    let novel_names: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM attribute_names WHERE name LIKE $1")
        .bind(format!("%{novel_term}"))
        .fetch_one(&pool)
        .await
        .expect("the dictionary must be readable");
    let repeated = client
        .post_entity(&entity_with_attribute(&repeat_entity, &established_term))
        .send()
        .await
        .expect("create entity request failed");

    // Every observation is taken before the dictionary is put back and
    // asserted afterwards, so a failed assertion cannot leave eleven thousand
    // generated names behind in the shared dictionary.
    discard(client, &established_entity).await;
    discard(client, &novel_entity).await;
    discard(client, &repeat_entity).await;
    remove_generated_names(&pool).await;
    pool.close().await;

    assert!(
        size > i64::from(NAMES_INTERNED_FIRST),
        "the dictionary must be past ten thousand names for this check to mean anything, size was {size}"
    );
    assert_eq!(novel.status, StatusCode::CREATED, "a new name is minted at any size: {:?}", novel.body);
    assert_eq!(novel_names, 1, "the new name is recorded exactly once");
    assert_eq!(repeated.status, StatusCode::CREATED, "an existing name stays usable: {:?}", repeated.body);

    environment.finish().await.expect("releasing exclusive ownership failed");
}
