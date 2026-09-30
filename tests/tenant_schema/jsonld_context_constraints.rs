//! The integrity the stored JSON-LD contexts enforce on their own.
//!
//! A `Cached` context is by definition a copy of a document fetched from a
//! remote URL (clause 5.2.19), and the cache is keyed and refreshed by that
//! URL. A cached row without one could never be refreshed nor found again by
//! the dereferencing that created it, so the check refuses it however the row
//! was written; the kinds the broker serves itself need no URL.
//!
//! A download also records the URL it was finally read from, after redirects
//! and a JSON-LD alternate link, because its own relative references resolve
//! against that URL. Only a download has one: a document the broker stores
//! itself was never read from anywhere, so the check ties the column to the
//! `Cached` kind in both directions.

use crate::SUITE;
use andromeda_postgresql_codes::jsonld_context_kind::JsonldContextKindCode;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use sqlx::PgPool;
use std::borrow::Cow;

/// The SQLSTATE PostgreSQL raises for a violated check constraint.
const CHECK_VIOLATION: &str = "23514";

/// The columns of one context row the checks read.
struct ContextRow<'a> {
    kind: JsonldContextKindCode,
    id: &'a str,
    url: Option<&'a str>,
    document_url: Option<&'a str>,
}

/// Insert one context row inside a transaction that is always rolled back, and
/// return what the database made of it.
async fn insert_context(pool: &PgPool, row: ContextRow<'_>) -> Result<(), sqlx::Error> {
    let mut transaction = pool.begin().await.expect("transaction");
    let outcome = sqlx::query("INSERT INTO jsonld_contexts (kind, id, url, document_url, payload) VALUES ($1, $2, $3, $4, '{\"@context\": {}}'::jsonb)")
        .bind(row.kind.code())
        .bind(row.id)
        .bind(row.url)
        .bind(row.document_url)
        .execute(&mut *transaction)
        .await;
    transaction.rollback().await.expect("rollback");
    outcome.map(|_| ())
}

/// Assert `refused` is a check violation of `constraint`.
fn assert_check_violation(refused: Result<(), sqlx::Error>, constraint: &str) {
    let violation = refused.expect_err("the row must be refused");
    let database_error = violation.as_database_error().expect("the refusal comes from the database");
    assert_eq!(
        database_error.code().map(Cow::into_owned).as_deref(),
        Some(CHECK_VIOLATION),
        "the refusal must be a check violation: {violation}"
    );
    assert_eq!(database_error.constraint(), Some(constraint), "{violation}");
}

#[tokio::test]
async fn tenant_schema_a_cached_context_without_a_url_is_refused() {
    let scope = TestEnvironment::from_suite(SUITE).await.expect("isolated test environment");
    let pool = scope.database_pool();
    let id = scope.id("JsonldContext:cached-without-url");
    let url = format!("http://127.0.0.1:9/contexts/{id}");

    let refused = insert_context(
        pool,
        ContextRow {
            kind: JsonldContextKindCode::Cached,
            id: &id,
            url: None,
            document_url: Some(&url),
        },
    )
    .await;
    assert_check_violation(refused, "chk_jsonld_contexts_cached_has_url");

    // The check is exactly the cached-has-URL rule: the same row with a URL is
    // stored, and a context the broker serves itself is stored without one.
    let with_url = insert_context(
        pool,
        ContextRow {
            kind: JsonldContextKindCode::Cached,
            id: &id,
            url: Some(&url),
            document_url: Some(&url),
        },
    )
    .await;
    assert!(with_url.is_ok(), "a cached context with a URL must be storable: {with_url:?}");
    let hosted = insert_context(
        pool,
        ContextRow {
            kind: JsonldContextKindCode::Hosted,
            id: &id,
            url: None,
            document_url: None,
        },
    )
    .await;
    assert!(hosted.is_ok(), "a hosted context needs no URL: {hosted:?}");

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn tenant_schema_a_cached_context_without_the_url_it_was_read_from_is_refused() {
    let scope = TestEnvironment::from_suite(SUITE).await.expect("isolated test environment");
    let pool = scope.database_pool();
    let id = scope.id("JsonldContext:cached-without-document-url");
    let url = format!("http://127.0.0.1:9/contexts/{id}");

    let refused = insert_context(
        pool,
        ContextRow {
            kind: JsonldContextKindCode::Cached,
            id: &id,
            url: Some(&url),
            document_url: None,
        },
    )
    .await;
    assert_check_violation(refused, "chk_jsonld_contexts_document_url_kind");

    // A redirected download records where it finally came from, which need not
    // be the URL the row is keyed by.
    let redirected = format!("http://127.0.0.1:9/contexts/v2/{id}");
    let stored = insert_context(
        pool,
        ContextRow {
            kind: JsonldContextKindCode::Cached,
            id: &id,
            url: Some(&url),
            document_url: Some(&redirected),
        },
    )
    .await;
    assert!(stored.is_ok(), "a cached context with the URL it was read from must be storable: {stored:?}");

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn tenant_schema_a_context_the_broker_stores_itself_records_no_document_url() {
    let scope = TestEnvironment::from_suite(SUITE).await.expect("isolated test environment");
    let pool = scope.database_pool();
    let id = scope.id("JsonldContext:stored-with-document-url");
    let url = format!("http://127.0.0.1:9/ngsi-ld/v1/jsonldContexts/{id}");

    for kind in [JsonldContextKindCode::Hosted, JsonldContextKindCode::ImplicitlyCreated] {
        let refused = insert_context(
            pool,
            ContextRow {
                kind,
                id: &id,
                url: Some(&url),
                document_url: Some(&url),
            },
        )
        .await;
        assert_check_violation(refused, "chk_jsonld_contexts_document_url_kind");
    }

    scope.finish().await.expect("cleanup failed");
}
