//! Attribute dictionary growth limit: minting new attribute names is bounded.
//!
//! `attribute_names` is append-only and attribute IRIs are open-world (clause
//! 4.5.2), so any client can inflate the dictionary indefinitely. The bound is
//! enforced above the database, in the request path that mints names, and it
//! bounds creation only: a tenant whose dictionary is already at its bound must
//! keep serving writes of the names it already holds.

use crate::{GENERATED_NAME_PREFIX, SUITE};
use andromeda_test_client::test_client::TestClient;
use andromeda_test_fixtures::context::CORE_CONTEXT;
use andromeda_test_harness::{configuration::TestRunConfiguration, database_access::connect_test_database, serialized_environment::SerializedEnvironment};
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

/// Names interned to push the dictionary past the bound the broker enforces.
///
/// `DictionaryNameLimit::DEFAULT` is 10,000 entries, and the guard refuses a
/// batch that would cross it, so a dictionary this size refuses every further
/// name whatever else the tenant already holds.
const NAMES_PAST_THE_BOUND: i32 = 11_000;

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
async fn a_dictionary_at_its_bound_refuses_new_names_and_still_serves_the_names_it_holds() {
    let configuration = TestRunConfiguration::from_environment().expect("integration test configuration");
    let environment = SerializedEnvironment::acquire(&configuration)
        .await
        .expect("exclusive ownership of the shared dictionary");
    let client = environment.client();
    let pool = connect_test_database().await.expect("test database connection");
    let run = Uuid::new_v4();
    let established_term = format!("dictionaryBound{}", run.simple());
    let novel_term = format!("dictionaryNovel{}", run.simple());
    let established_entity = format!("urn:ngsi-ld:test:{SUITE}:Device:established:{run}");
    let novel_entity = format!("urn:ngsi-ld:test:{SUITE}:Device:novel:{run}");
    let repeat_entity = format!("urn:ngsi-ld:test:{SUITE}:Device:repeat:{run}");

    remove_generated_names(&pool).await;

    // The name has to exist before the bound is reached, because reaching the
    // bound is exactly what stops it from being minted later.
    let established = client
        .post_entity(&entity_with_attribute(&established_entity, &established_term))
        .send()
        .await
        .expect("create entity request failed");
    assert_eq!(established.status, StatusCode::CREATED, "create: {:?}", established.body);

    sqlx::query("INSERT INTO attribute_names (name) SELECT $1 || generated::TEXT FROM generate_series(1, $2) AS generated ON CONFLICT (name) DO NOTHING")
        .bind(GENERATED_NAME_PREFIX)
        .bind(NAMES_PAST_THE_BOUND)
        .execute(&pool)
        .await
        .expect("interning distinct attribute names must not fail");

    // The size is observable from the dictionary itself, which is what an
    // operator deciding between raising the bound and finding the client reads.
    let size: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM attribute_names")
        .fetch_one(&pool)
        .await
        .expect("the dictionary size must be observable");

    let refused = client
        .post_entity(&entity_with_attribute(&novel_entity, &novel_term))
        .send()
        .await
        .expect("create entity request failed");
    let novel_names: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM attribute_names WHERE name LIKE $1")
        .bind(format!("%{novel_term}"))
        .fetch_one(&pool)
        .await
        .expect("the dictionary must be readable");

    // A write that resolves only names the dictionary already holds creates
    // nothing, so the bound does not apply to it.
    let repeated = client
        .post_entity(&entity_with_attribute(&repeat_entity, &established_term))
        .send()
        .await
        .expect("create entity request failed");

    // Every observation this check makes is taken before the dictionary is put
    // back, and asserted afterwards. An assertion that failed while the
    // inflated dictionary was still in place would leave every later write on
    // this tenant refused for a name it was entitled to mint.
    discard(client, &established_entity).await;
    discard(client, &novel_entity).await;
    discard(client, &repeat_entity).await;
    remove_generated_names(&pool).await;
    pool.close().await;

    // The size is observable from the dictionary itself, which is what an
    // operator deciding between raising the bound and finding the client reads.
    assert!(
        size > i64::from(NAMES_PAST_THE_BOUND),
        "the dictionary must be past the bound for this check to mean anything, size was {size}"
    );
    assert_ne!(
        refused.status,
        StatusCode::CREATED,
        "a name minted past the bound must be refused: {:?}",
        refused.body
    );
    assert_eq!(novel_names, 0, "a refused request must mint no part of its batch");
    assert_eq!(
        repeated.status,
        StatusCode::CREATED,
        "an existing name stays usable at the bound: {:?}",
        repeated.body
    );

    environment.finish().await.expect("releasing exclusive ownership failed");
}
