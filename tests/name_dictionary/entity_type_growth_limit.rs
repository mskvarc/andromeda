//! Entity type dictionary growth limit: minting new type names is bounded.
//!
//! `entity_type_names` is append-only and entity type IRIs are open-world
//! (clause 4.5.1), so any client can inflate the dictionary indefinitely
//! exactly as it can the attribute dictionary. The bound is enforced above the
//! database by the same cache that bounds attribute names, and it bounds
//! creation only: a tenant whose type dictionary is already at its bound must
//! keep serving writes of the types it already holds.

use crate::{GENERATED_NAME_PREFIX, SUITE};
use andromeda_test_client::test_client::TestClient;
use andromeda_test_fixtures::context::CORE_CONTEXT;
use andromeda_test_harness::{configuration::TestRunConfiguration, database_access::connect_test_database, serialized_environment::SerializedEnvironment};
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

/// Types interned to push the dictionary past the bound the broker enforces.
///
/// `DictionaryNameLimit::DEFAULT` is 10,000 entries per dictionary, and the
/// guard refuses a batch that would cross it, so a dictionary this size refuses
/// every further type whatever else the tenant already holds.
const TYPES_PAST_THE_BOUND: i32 = 11_000;

/// An entity of one type under the core context's default vocabulary.
fn entity_of_type(id: &str, entity_type: &str) -> Value {
    json!({
        "id": id,
        "type": entity_type,
        "@context": [CORE_CONTEXT]
    })
}

/// Remove exactly the type names this check interned.
async fn remove_generated_types(pool: &PgPool) {
    sqlx::query("DELETE FROM entity_type_names WHERE name LIKE $1")
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
async fn a_type_dictionary_at_its_bound_refuses_new_types_and_still_serves_the_types_it_holds() {
    let configuration = TestRunConfiguration::from_environment().expect("integration test configuration");
    let environment = SerializedEnvironment::acquire(&configuration)
        .await
        .expect("exclusive ownership of the shared dictionary");
    let client = environment.client();
    let pool = connect_test_database().await.expect("test database connection");
    let run = Uuid::new_v4();
    let established_type = format!("TypeBound{}", run.simple());
    let novel_type = format!("TypeNovel{}", run.simple());
    let established_entity = format!("urn:ngsi-ld:test:{SUITE}:Type:established:{run}");
    let novel_entity = format!("urn:ngsi-ld:test:{SUITE}:Type:novel:{run}");
    let repeat_entity = format!("urn:ngsi-ld:test:{SUITE}:Type:repeat:{run}");

    remove_generated_types(&pool).await;

    // The type has to exist before the bound is reached, because reaching the
    // bound is exactly what stops it from being minted later.
    let established = client
        .post_entity(&entity_of_type(&established_entity, &established_type))
        .send()
        .await
        .expect("create entity request failed");
    assert_eq!(established.status, StatusCode::CREATED, "create: {:?}", established.body);

    sqlx::query("INSERT INTO entity_type_names (name) SELECT $1 || generated::TEXT FROM generate_series(1, $2) AS generated ON CONFLICT (name) DO NOTHING")
        .bind(GENERATED_NAME_PREFIX)
        .bind(TYPES_PAST_THE_BOUND)
        .execute(&pool)
        .await
        .expect("interning distinct type names must not fail");

    let size: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM entity_type_names")
        .fetch_one(&pool)
        .await
        .expect("the dictionary size must be observable");

    let refused = client
        .post_entity(&entity_of_type(&novel_entity, &novel_type))
        .send()
        .await
        .expect("create entity request failed");
    let novel_types: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM entity_type_names WHERE name LIKE $1")
        .bind(format!("%{novel_type}"))
        .fetch_one(&pool)
        .await
        .expect("the dictionary must be readable");

    // A write that resolves only types the dictionary already holds creates
    // nothing, so the bound does not apply to it.
    let repeated = client
        .post_entity(&entity_of_type(&repeat_entity, &established_type))
        .send()
        .await
        .expect("create entity request failed");

    // Every observation is taken before the dictionary is put back and
    // asserted afterwards, so a failed assertion cannot leave every later
    // write on this tenant refused for a type it was entitled to mint.
    discard(client, &established_entity).await;
    discard(client, &novel_entity).await;
    discard(client, &repeat_entity).await;
    remove_generated_types(&pool).await;
    pool.close().await;

    assert!(
        size > i64::from(TYPES_PAST_THE_BOUND),
        "the dictionary must be past the bound for this check to mean anything, size was {size}"
    );
    assert_ne!(
        refused.status,
        StatusCode::CREATED,
        "a type minted past the bound must be refused: {:?}",
        refused.body
    );
    assert_eq!(novel_types, 0, "a refused request must mint no part of its batch");
    assert_eq!(
        repeated.status,
        StatusCode::CREATED,
        "an existing type stays usable at the bound: {:?}",
        repeated.body
    );

    environment.finish().await.expect("releasing exclusive ownership failed");
}
