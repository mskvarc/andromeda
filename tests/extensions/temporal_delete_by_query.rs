//! `DELETE /temporal/entities` — the temporal history delete-by-query extension.
//!
//! Runs against a broker started with `ANDROMEDA__EXTENSIONS__TEMPORAL_DELETE_BY_QUERY=enabled`
//! and a small `ANDROMEDA__EXTENSIONS__TEMPORAL_DELETE_BY_QUERY_MAX_MATCHES` (see
//! `docker/extensions/extensions.env`).

use crate::seeding::{T3, seed_three_instant_history, surviving_speed_instants};
use andromeda_test_client::{header::jsonld_link_header, response::TestResponse, test_client::TestClient};
use andromeda_test_fixtures::context::CORE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};

/// Issue a `DELETE /temporal/entities` with the given query string.
async fn delete_temporal_by_query(client: &TestClient, query: &str) -> TestResponse {
    let path = format!("/ngsi-ld/v1/temporal/entities?{query}");
    client
        .raw(Method::DELETE, &path)
        .header("Link", jsonld_link_header(CORE_CONTEXT).expect("core context Link"))
        .send()
        .await
        .expect("temporal delete-by-query request")
}

/// The window `[T1, T3)` removes the two earlier instants and leaves `T3`,
/// which the half-open upper bound excludes.
#[tokio::test]
async fn a_between_window_removes_its_instances_and_leaves_the_excluded_upper_bound() {
    let scope = TestEnvironment::from_suite("extensions").await.expect("isolated environment");
    let client = scope.client();
    let id = scope.id("Vehicle:window");
    seed_three_instant_history(client, &id).await;

    let query = format!(
        "id={}&drop=speed&timerel=between&timeAt=2024-01-01T00:00:00Z&endTimeAt=2024-06-01T00:00:00Z",
        urlencoding::encode(&id)
    );
    let response = delete_temporal_by_query(client, &query).await;
    assert_eq!(response.status, StatusCode::NO_CONTENT, "a real delete answers 204");

    let survivors = surviving_speed_instants(client, &id).await;
    assert_eq!(survivors, vec![T3.to_string()], "only the excluded upper-bound instant survives");
    scope.finish().await.expect("cleanup");
}

/// A dry run reports the would-delete count and removes nothing.
#[tokio::test]
async fn a_dry_run_reports_the_count_and_removes_nothing() {
    let scope = TestEnvironment::from_suite("extensions").await.expect("isolated environment");
    let client = scope.client();
    let id = scope.id("Vehicle:dryrun");
    seed_three_instant_history(client, &id).await;

    let query = format!(
        "id={}&drop=speed&timerel=between&timeAt=2024-01-01T00:00:00Z&endTimeAt=2024-06-01T00:00:00Z&dryRun=true",
        urlencoding::encode(&id)
    );
    let response = delete_temporal_by_query(client, &query).await;
    assert_eq!(response.status, StatusCode::OK, "a dry run answers 200");
    let body = response.json_value().expect("dry-run body is JSON");
    assert_eq!(body["deleted"], 2, "the window matches the two earlier instants");

    let survivors = surviving_speed_instants(client, &id).await;
    assert_eq!(survivors.len(), 3, "a dry run removes nothing");
    scope.finish().await.expect("cleanup");
}

/// An unbounded delete over more instances than the configured bound is refused
/// with `TooManyResults`, having removed nothing.
#[tokio::test]
async fn an_over_the_bound_delete_is_refused_without_removing_anything() {
    let scope = TestEnvironment::from_suite("extensions").await.expect("isolated environment");
    let client = scope.client();
    let id = scope.id("Vehicle:toomany");
    seed_three_instant_history(client, &id).await;

    let query = format!("id={}&drop=speed", urlencoding::encode(&id));
    let response = delete_temporal_by_query(client, &query).await;
    assert_eq!(response.status, StatusCode::FORBIDDEN, "an over-the-bound delete answers 403 TooManyResults");

    let survivors = surviving_speed_instants(client, &id).await;
    assert_eq!(survivors.len(), 3, "a refused delete removes nothing");
    scope.finish().await.expect("cleanup");
}

/// A bare time window authorises deleting every entity's history, so it is
/// refused as a too-wide request.
#[tokio::test]
async fn a_window_only_delete_is_rejected_as_too_wide() {
    let scope = TestEnvironment::from_suite("extensions").await.expect("isolated environment");
    let client = scope.client();
    let response = delete_temporal_by_query(client, "timerel=before&timeAt=2030-01-01T00:00:00Z").await;
    assert_eq!(response.status, StatusCode::BAD_REQUEST, "a window with no selector is too wide");
    scope.finish().await.expect("cleanup");
}

/// The enabled extension is advertised in the Context Source Identity response.
#[tokio::test]
async fn the_extension_is_advertised_in_the_source_identity() {
    let scope = TestEnvironment::from_suite("extensions").await.expect("isolated environment");
    let client = scope.client();
    let response = client.source_identity(None).await.expect("source identity");
    assert_eq!(response.status, StatusCode::OK);
    let body = response.json_value().expect("identity body is JSON");
    let advertised = body.to_string();
    assert!(
        advertised.contains("temporal-delete-by-query"),
        "the enabled extension is advertised: {advertised}"
    );
    scope.finish().await.expect("cleanup");
}
