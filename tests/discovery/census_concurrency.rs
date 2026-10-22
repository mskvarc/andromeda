#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]
//! The census under concurrent writers in two broker processes.
//!
//! One broker serialises writes to one entity in process, so only two brokers
//! over one database make two writers race on the same `entity_search` row.
//! That is where a statement deriving the census or a type membership from its
//! own snapshot of the row, rather than from the row its write actually
//! replaced, would count a change twice or not at all, keep a membership the
//! other writer dropped, or collide on one it added. Writers on both brokers
//! rewrite the same few entities with changing type sets and attribute shapes,
//! and afterwards the census equals its recomputation, every type membership
//! matches the types the search row holds, and discovery answers what the
//! entities hold.

use crate::{
    BASE_URL,
    census_random::{ATTRIBUTES, TYPES, discovered, expected},
    client,
    provision_tenant,
};
use andromeda_postgresql_schema::current::{EntitySearch, EntityTypeMembers};
use andromeda_postgresql_seaquery::{aggregate::count_all, statement::Statement, unnest::unnest_of};
use andromeda_test_harness::{discovery_census::census_drift, tenant_scoped_environment::TenantScopedEnvironment};
use futures::future::join_all;
use reqwest::{Client, Method, StatusCode};
use sea_query::{Alias, Expr, Query, UnionType};
use serde_json::{Map, Value, json};
use std::env::var;

/// Names the second broker of the tier, which serves the same database.
const SECOND_BROKER_ENV: &str = "ANDROMEDA_TEST_TRUSTED_PROXY_BROKER_URL";

/// The second broker's API root when [`SECOND_BROKER_ENV`] names no other.
const DEFAULT_SECOND_BROKER: &str = "http://127.0.0.1:1057/ngsi-ld/v1/";

const ENTITIES: usize = 6;
const WRITERS_PER_BROKER: usize = 6;
const ROUNDS: usize = 20;

/// One of a few type sets, chosen by `round`, so concurrent writers keep
/// adding and dropping the same types.
fn types_for(round: usize) -> Value {
    match round % 4 {
        0 => json!(TYPES[0]),
        1 => json!([TYPES[0], TYPES[1]]),
        2 => json!([TYPES[1], TYPES[2]]),
        _ => json!([TYPES[2], TYPES[0], TYPES[3]]),
    }
}

/// One of a few attribute shapes, chosen by `round`: which attributes, which
/// families and how many `datasetId` instances.
fn members_for(round: usize) -> Map<String, Value> {
    let mut members = Map::new();
    members.insert(ATTRIBUTES[0].to_owned(), json!({"type": "Property", "value": round}));
    if round.is_multiple_of(2) {
        members.insert(
            ATTRIBUTES[1].to_owned(),
            json!([{"type": "Relationship", "object": "urn:ngsi-ld:Concurrent:x"}, {"type": "Property", "value": 1, "datasetId": "urn:ngsi-ld:Dataset:one"}]),
        );
    }
    if round.is_multiple_of(3) {
        members.insert(
            ATTRIBUTES[2].to_owned(),
            json!({"type": "GeoProperty", "value": {"type": "Point", "coordinates": [14.5, 46.0]}}),
        );
    }
    members
}

fn entity(id: &str, round: usize) -> Value {
    let mut members = members_for(round);
    members.insert("id".to_owned(), Value::String(id.to_owned()));
    members.insert("type".to_owned(), types_for(round));
    Value::Object(members)
}

async fn send(c: &Client, base: &str, tenant: &str, method: Method, path: &str, body: &Value) -> StatusCode {
    c.request(method, format!("{}/{}", base.trim_end_matches('/'), path.trim_start_matches('/')))
        .header("NGSILD-Tenant", tenant)
        .header("Content-Type", "application/json")
        .json(body)
        .send()
        .await
        .expect("request failed")
        .status()
}

/// One writer: rounds of batch upsert replacements, single replacements,
/// type-adding merges, deletions and re-creations over every entity.
async fn writer(base: String, tenant: String, ids: Vec<String>, offset: usize) {
    let c = client();
    for round in 0..ROUNDS {
        let turn = round + offset;
        let status = match turn % 5 {
            0 | 1 => {
                let batch: Vec<Value> = ids.iter().map(|id| entity(id, turn)).collect();
                send(
                    &c,
                    &base,
                    &tenant,
                    Method::POST,
                    "/entityOperations/upsert?options=replace",
                    &Value::Array(batch),
                )
                .await
            }
            2 => {
                let id = &ids[turn % ids.len()];
                let mut replacement = entity(id, turn + 1);
                replacement.as_object_mut().expect("an entity object").remove("id");
                send(&c, &base, &tenant, Method::PUT, &format!("/entities/{id}"), &replacement).await
            }
            3 => {
                let id = &ids[turn % ids.len()];
                send(
                    &c,
                    &base,
                    &tenant,
                    Method::PATCH,
                    &format!("/entities/{id}"),
                    &json!({"type": types_for(turn + 2)}),
                )
                .await
            }
            _ => {
                let id = &ids[turn % ids.len()];
                let deleted = send(&c, &base, &tenant, Method::DELETE, &format!("/entities/{id}"), &Value::Null).await;
                let created = send(&c, &base, &tenant, Method::POST, "/entities", &entity(id, turn)).await;
                if deleted.is_server_error() { deleted } else { created }
            }
        };
        assert!(!status.is_server_error(), "a concurrent write answered {status}");
    }
}

#[tokio::test]
async fn discovery_census_stays_exact_under_writers_in_two_brokers() {
    let env = TenantScopedEnvironment::from_suite("discovery").await.expect("private tenant environment");
    let c = client();
    let tenant = env.tenant().to_owned();
    provision_tenant(&c, &tenant).await;
    let second = var(SECOND_BROKER_ENV).unwrap_or_else(|_| DEFAULT_SECOND_BROKER.to_owned());
    let first = BASE_URL.to_string();
    let ids: Vec<String> = (0..ENTITIES).map(|n| env.id(&format!("concurrent:{n}"))).collect();
    let seeded: Vec<Value> = ids.iter().map(|id| entity(id, 0)).collect();
    pretty_assertions::assert_eq!(
        send(&c, &first, &tenant, Method::POST, "/entityOperations/create", &Value::Array(seeded)).await,
        StatusCode::CREATED
    );

    let writers = (0..WRITERS_PER_BROKER).flat_map(|n| {
        [
            tokio::spawn(writer(first.clone(), tenant.clone(), ids.clone(), n)),
            tokio::spawn(writer(second.clone(), tenant.clone(), ids.clone(), n + 1)),
        ]
    });
    for joined in join_all(writers).await {
        joined.expect("a writer panicked");
    }

    let pool = env.tenant_database().await.expect("the tenant database");
    let drift = census_drift(&pool, "public").await.expect("the census comparison");
    assert!(drift.is_empty(), "the census drifted under concurrent writers: {drift:?}");
    let memberships = || {
        Query::select()
            .columns([EntityTypeMembers::EntityKey, EntityTypeMembers::EntityTypeNameKey])
            .from(EntityTypeMembers::Table)
            .to_owned()
    };
    let searched = || {
        Query::select()
            .column(EntitySearch::EntityKey)
            .expr(unnest_of(Expr::col(EntitySearch::EntityTypeNameKeys)))
            .from(EntitySearch::Table)
            .to_owned()
    };
    let differing = memberships()
        .union(UnionType::Except, searched())
        .union(UnionType::All, searched().union(UnionType::Except, memberships()).to_owned())
        .to_owned();
    let stray_memberships: i64 = Query::select()
        .expr(count_all())
        .from_subquery(differing, Alias::new("differing"))
        .to_query_scalar()
        .fetch_one(&pool)
        .await
        .expect("the membership comparison");
    pretty_assertions::assert_eq!(stray_memberships, 0, "every type membership matches the types its search row holds");
    pool.close().await;
    pretty_assertions::assert_eq!(discovered(&c, &tenant).await, expected(&c, &tenant).await);
    env.finish().await.expect("cleanup failed");
}
