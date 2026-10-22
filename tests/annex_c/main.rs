//! Replay of the worked examples in ETSI GS CIM 009 v1.9.1 annex C against a live broker.
//!
//! Annex C is informative: it prints request/response exchanges and payload
//! shapes that a conforming implementation should be able to produce and
//! accept. Each example in clauses C.2 to C.5 is replayed here as its own named
//! test, and each behavioural clarification in clauses C.6 to C.11 is turned
//! into an assertion about what the broker actually does. A failing test names
//! the clause, so the failure reads as "clause C.2.3's concise `GeoJSON` example
//! does not come back as printed" rather than as an anonymous mismatch.
//!
//! # The comparison rule
//!
//! A response cannot be compared to a printed example byte for byte, and a
//! comparison loose enough to survive that is a comparison that cannot fail.
//! The rule this suite applies is deliberately narrow.
//!
//! 1. **Broker-minted timestamps and identities are erased, and only those.**
//!    `createdAt`, `modifiedAt`, `deletedAt` and `instanceId` are removed
//!    recursively from the broker's answer before comparison, because they are
//!    generated at write time and no printed example can state them. Nothing
//!    else is removed. An extra member the broker adds and the example does not
//!    print is a difference and fails the test.
//!
//! 2. **`@context` is not compared as data outside the clauses about it.**
//!    In C.2 to C.5 the printed `@context` is whatever the example's author
//!    chose to write; what a broker renders is decided by how the client
//!    supplied the context on that request, which is exactly what clauses C.7,
//!    C.8 and C.9 exist to demonstrate. Those three clauses assert on the
//!    rendered `@context` directly; every other test strips the member from
//!    both sides.
//!
//! 3. **Example vocabulary URLs are canonicalized.** The suite hosts the
//!    `commonTerms`, `vehicle`, `parking` and wrapper `@context` documents of
//!    clause C.2.4 on an ephemeral local port, so before comparison any
//!    occurrence of that origin is rewritten back to the
//!    `http://example.org/ngsi-ld/latest/` prefix the annex prints.
//!
//! 4. **Object member order never matters; array order always matters, with
//!    one exception.** `valueList`, `objectList`, `entityList`, temporal
//!    `values`, `GeoJSON` `coordinates` and `features` all keep their order,
//!    because ordering is the thing those members exist to express. The
//!    exception is the top level of a query answer, where clause 5.7.2 mandates
//!    no order unless `orderBy` was given: there both sides are sorted by `id`
//!    first.
//!
//! 5. **Numbers compare by JSON value.** `41.2` equals `41.20`.
//!
//! 6. **Everything else is exact deep equality.** A missing member, an extra
//!    member, a different spelling and a different JSON type all fail.
//!
//! # The corpus
//!
//! Expected payloads live beside this file as JSON documents under `corpus/`,
//! transcribed from the annex rather than restated in Rust. Keeping them as
//! data makes each one diffable against the printed example, which matters
//! because several printed examples are not valid JSON.
//!
//! Naming convention: `annex_c_<clause>_<example>`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

mod comparison;
mod context_precedence;
mod context_source_registration;
mod context_subscription;
mod context_utilization;
mod date_representation;
mod digital_signature;
mod example_context;
mod link_header_utilization;
mod merged_context;
mod parking_entity;
mod rest_discovery;
mod rest_entity_operations;
mod rest_temporal_queries;
mod value_type_utilization;
mod vehicle_entity;

use andromeda_postgresql_seaquery::{
    command::Command,
    database_definition::{ConnectedSessions, DatabaseDefinition},
    statement::Statement,
    system_catalog::PgDatabase,
};
use andromeda_test_harness::{
    configuration::{RUNTIME_BROKER_BASE as BASE_URL, TestRunConfiguration},
    database_isolation::connect_pool,
    tenant_scoped_environment::sweep_stale_tenant_databases_once,
};
use pretty_assertions::assert_eq;
use reqwest::{
    Client,
    RequestBuilder,
    StatusCode,
    header::{HeaderMap, HeaderValue},
};
use sea_query::{Alias, Expr, ExprTrait, Query};
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

/// A private-tenant environment: an HTTP client bound to a unique tenant plus the
/// control-database pool that reclaims that tenant on `finish`.
///
/// Annex C examples name fixed Entity identifiers -- `urn:ngsi-ld:Vehicle:A4567`
/// and friends -- so replaying two of them against a shared tenant would have
/// them read each other's fixtures. Each test runs against a tenant of its own,
/// carried on every request by an `NGSILD-Tenant` default header, so the fixed
/// identifiers and whole-response comparisons stay valid and the suite
/// parallelizes without inventing identifiers the annex does not print.
struct AnnexTenant {
    http: Client,
    label: String,
    pool: PgPool,
}

impl AnnexTenant {
    /// Allocate a uniquely named tenant and a client that addresses it.
    async fn allocate() -> AnnexTenant {
        let configuration = TestRunConfiguration::from_environment().expect("test configuration");
        let pool = connect_pool(configuration.database()).await.expect("control database pool");
        // Reclaim tenant databases a previous suite's force-drops missed before
        // this suite adds its own, so their connection slots do not accumulate
        // across the tier.
        sweep_stale_tenant_databases_once(&pool).await;
        // A database-safe, lowercase label so the broker's tenant database is
        // `andromeda_tenant_<label>_<suffix>` and finish can find it by prefix.
        let label = format!("annexc{}", &Uuid::new_v4().simple().to_string()[..12]);
        let mut headers = HeaderMap::new();
        headers.insert("NGSILD-Tenant", HeaderValue::from_str(&label).expect("tenant label is a valid header value"));
        let http = Client::builder().default_headers(headers).build().expect("tenant-bound HTTP client");
        AnnexTenant { http, label, pool }
    }

    /// A tenant-bound HTTP client. The clone is an `Arc` bump, and returning an
    /// owned client lets a test hold it across the `finish` that consumes `self`.
    fn client(&self) -> Client {
        self.http.clone()
    }

    /// Force-drop the tenant's databases and close the pool; best-effort, because
    /// the unique label means a leftover never collides with another test.
    async fn finish(self) {
        // Suspend the tenant so the broker closes its runtime pool before the
        // database is dropped; dropping it out from under a cached runtime makes
        // the next tenant provisioned answer 503 until the janitor evicts the
        // stale entry. Best-effort.
        let _ = self
            .http
            .patch(format!("{BASE_URL}/tenants/{}", self.label))
            .header("content-type", "application/json")
            .body("{\"isActive\":false}")
            .send()
            .await;
        let like = format!("andromeda_tenant_{}_%", self.label);
        if let Ok(databases) = Query::select()
            .column(PgDatabase::Datname)
            .from(PgDatabase::Table)
            .and_where(Expr::col(PgDatabase::Datname).like(like))
            .to_query_scalar::<String>()
            .fetch_all(&self.pool)
            .await
        {
            for database in databases {
                // `WITH (FORCE)` evicts the tenant's background workers that hold
                // the database open; the name comes from `pg_database`.
                let _ = DatabaseDefinition::drop(Alias::new(database), ConnectedSessions::Terminate)
                    .to_query()
                    .execute(&self.pool)
                    .await;
            }
        }
        self.pool.close().await;
    }
}

/// Allocate a private tenant whose collections start empty.
async fn clean_tenant() -> AnnexTenant {
    AnnexTenant::allocate().await
}

/// Create one Entity through clause 5.6.1 and assert it was accepted.
async fn create_entity(http: &Client, entity: &Value, link: Option<&str>) {
    let response = write_request(http, &format!("{BASE_URL}/entities/"), entity, link)
        .send()
        .await
        .expect("entity creation request");
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    assert_eq!(status, StatusCode::CREATED, "entity creation refused: {body}");
}

/// Create a batch of Entities through clause 5.6.7 and assert every one landed.
async fn create_entity_batch(http: &Client, entities: &Value, link: Option<&str>) {
    let response = write_request(http, &format!("{BASE_URL}/entityOperations/create"), entities, link)
        .send()
        .await
        .expect("batch creation request");
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    assert_eq!(status, StatusCode::CREATED, "batch creation refused: {body}");
}

/// Build a write request that carries its `@context` exactly one way.
///
/// Clause 5.5.7 refuses `application/ld+json` combined with a `Link` context
/// header, so a caller that names a hosted `@context` sends `application/json`
/// and the payload's own printed `@context` is dropped. A caller that names
/// none sends the payload as JSON-LD with the `@context` it prints.
fn write_request(http: &Client, url: &str, payload: &Value, link: Option<&str>) -> RequestBuilder {
    match link {
        Some(link) => http
            .post(url)
            .header("Content-Type", "application/json")
            .header("Link", link)
            .json(&without_context(payload)),
        None => http.post(url).header("Content-Type", "application/ld+json").json(payload),
    }
}

/// Drop the `@context` member of an Entity, or of every Entity in a batch.
fn without_context(payload: &Value) -> Value {
    match payload {
        Value::Object(members) => Value::Object(
            members
                .iter()
                .filter(|(key, _)| key.as_str() != "@context")
                .map(|(key, value)| (key.clone(), value.clone())) // clone: the request body owns its members.
                .collect(),
        ),
        Value::Array(entities) => Value::Array(entities.iter().map(without_context).collect()),
        other @ (Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_)) => other.clone(), // clone: the request body owns its leaves.
    }
}

/// Issue one GET against the broker and return its status and parsed body.
async fn get_json(http: &Client, path: &str, accept: &str, link: Option<&str>) -> (StatusCode, Value) {
    let request = http.get(format!("{BASE_URL}{path}")).header("Accept", accept);
    let response = with_link(request, link).send().await.expect("retrieval request");
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    let parsed = serde_json::from_str(&body).unwrap_or(Value::Null);
    (status, parsed)
}

/// Issue one GET and return its status, headers and parsed body.
async fn get_with_headers(http: &Client, path: &str, accept: &str, link: Option<&str>) -> (StatusCode, HeaderMap, Value) {
    let request = http.get(format!("{BASE_URL}{path}")).header("Accept", accept);
    let response = with_link(request, link).send().await.expect("retrieval request");
    let status = response.status();
    let headers = response.headers().clone(); // clone: the headers outlive the response the body is read from.
    let body = response.text().await.unwrap_or_default();
    let parsed = serde_json::from_str(&body).unwrap_or(Value::Null);
    (status, headers, parsed)
}

/// Attach the JSON-LD `Link` header when the example supplies one.
fn with_link(request: RequestBuilder, link: Option<&str>) -> RequestBuilder {
    match link {
        Some(value) => request.header("Link", value),
        None => request,
    }
}

/// Render the `Link` header value clause C.5 prints for a hosted `@context`.
fn context_link(url: &str) -> String {
    format!("<{url}>; rel=\"http://www.w3.org/ns/json-ld#context\"; type=\"application/ld+json\"")
}
