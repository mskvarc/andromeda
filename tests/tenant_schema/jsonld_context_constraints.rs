//! The integrity the stored JSON-LD contexts enforce on their own.
//!
//! A `Cached` context is by definition a copy of a document fetched from a
//! remote URL (clause 5.2.19), and the cache is keyed and refreshed by that
//! URL. A cached row without one could never be refreshed nor found again by
//! the dereferencing that created it, so the check refuses it however the row
//! was written; the kinds the broker serves itself need no URL.

use crate::SUITE;
use andromeda_postgresql_codes::jsonld_context_kind::JsonldContextKindCode;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use sqlx::PgPool;
use std::borrow::Cow;

/// The SQLSTATE PostgreSQL raises for a violated check constraint.
const CHECK_VIOLATION: &str = "23514";

/// Insert one context row of `kind` inside a transaction that is always rolled
/// back, and return what the database made of it.
async fn insert_context(pool: &PgPool, kind: JsonldContextKindCode, id: &str, url: Option<&str>) -> Result<(), sqlx::Error> {
    let mut transaction = pool.begin().await.expect("transaction");
    let outcome = sqlx::query("INSERT INTO jsonld_contexts (kind, id, url, payload) VALUES ($1, $2, $3, '{\"@context\": {}}'::jsonb)")
        .bind(kind.code())
        .bind(id)
        .bind(url)
        .execute(&mut *transaction)
        .await;
    transaction.rollback().await.expect("rollback");
    outcome.map(|_| ())
}

#[tokio::test]
async fn tenant_schema_a_cached_context_without_a_url_is_refused() {
    let scope = TestEnvironment::from_suite(SUITE).await.expect("isolated test environment");
    let pool = scope.database_pool();
    let id = scope.id("JsonldContext:cached-without-url");

    let refused = insert_context(pool, JsonldContextKindCode::Cached, &id, None).await;
    let violation = refused.expect_err("a cached context without a URL must be refused");
    let database_error = violation.as_database_error().expect("the refusal comes from the database");
    assert_eq!(
        database_error.code().map(Cow::into_owned).as_deref(),
        Some(CHECK_VIOLATION),
        "the refusal must be a check violation: {violation}"
    );
    assert_eq!(database_error.constraint(), Some("chk_jsonld_contexts_cached_has_url"), "{violation}");

    // The check is exactly the cached-has-URL rule: the same row with a URL is
    // stored, and a context the broker serves itself is stored without one.
    let url = format!("http://127.0.0.1:9/contexts/{id}");
    let with_url = insert_context(pool, JsonldContextKindCode::Cached, &id, Some(&url)).await;
    assert!(with_url.is_ok(), "a cached context with a URL must be storable: {with_url:?}");
    let hosted = insert_context(pool, JsonldContextKindCode::Hosted, &id, None).await;
    assert!(hosted.is_ok(), "a hosted context needs no URL: {hosted:?}");

    scope.finish().await.expect("cleanup failed");
}
