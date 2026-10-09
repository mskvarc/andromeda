//! Hoisted geometry: a write that leaves `location` alone leaves its search
//! column alone.
//!
//! The search row holds the projection of the default `location` (clause
//! 4.7), which a geoQuery (clause 4.10) is answered from. Projecting it parses,
//! validates and compares every position, so an update of another attribute
//! (clause 5.6.2) must not project it again: the write stages the geometry as
//! kept and the row keeps the column it holds. To make that observable, the
//! stored column is replaced by a marker geometry the broker never wrote; a
//! write that re-projected the location would overwrite the marker.

use crate::{
    projection_rows::entity_key,
    write_consequences::{core_context, create, temporal_instances, write},
};
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::{Method, StatusCode};
use serde_json::json;
use sqlx::PgPool;
use urlencoding::encode;

/// The marker the search column is set to, which no request of this test
/// sends.
const MARKER: &str = "POINT(1 2)";

/// The stored search row of one Entity: its `location` as text, and the
/// transaction that last wrote the row version.
async fn search_row(pool: &PgPool, key: i64) -> (String, String) {
    sqlx::query_as("SELECT ST_AsText(location), xmin::text FROM entity_search WHERE entity_key = $1")
        .bind(key)
        .fetch_one(pool)
        .await
        .expect("the search row must be readable")
}

/// The transaction that last wrote the `location` row of the current-value
/// projection.
async fn location_projection_version(pool: &PgPool, key: i64) -> String {
    sqlx::query_scalar(
        "SELECT c.xmin::text FROM current_attributes AS c \
         JOIN attribute_names AS an ON an.attribute_name_key = c.attribute_name_key \
         WHERE c.entity_key = $1 AND an.name = 'https://uri.etsi.org/ngsi-ld/location'",
    )
    .bind(key)
    .fetch_one(pool)
    .await
    .expect("the location projection row must be readable")
}

#[tokio::test]
async fn current_projection_a_write_leaving_the_location_keeps_its_search_column() {
    let scope = TestEnvironment::from_suite("current-projection").await.expect("isolated test environment");
    let client = scope.client();
    let pool = scope.database_pool();
    let id = scope.id("Parcel:hoisted-geometry");
    create(
        client,
        &json!({
            "id": id,
            "type": scope.entity_type("Parcel"),
            "speed": {"type": "Property", "value": 1},
            "location": {"type": "GeoProperty", "value": {"type": "Polygon", "coordinates": [[[10.0, 50.0], [10.1, 50.0], [10.1, 50.1], [10.0, 50.0]]]}},
            "@context": core_context()
        }),
    )
    .await;
    let key = entity_key(pool, &id).await.expect("the created Entity has a row");
    sqlx::query("UPDATE entity_search SET location = ST_GeomFromText($1, 4326) WHERE entity_key = $2")
        .bind(MARKER)
        .bind(key)
        .execute(pool)
        .await
        .expect("the marker must be writable");
    let (_, marked_version) = search_row(pool, key).await;
    let location_version = location_projection_version(pool, key).await;
    let attrs = format!("entities/{}/attrs", encode(&id));

    // Clause 5.6.2: updating `speed` leaves `location` as it was, so neither
    // the search row nor the location's current-value row is rewritten, and
    // the location's Temporal Evolution gains no instance.
    write(
        client,
        Method::PATCH,
        &attrs,
        &json!({"speed": {"type": "Property", "value": 2}, "@context": core_context()}),
        StatusCode::NO_CONTENT,
    )
    .await;
    assert_eq!(
        search_row(pool, key).await,
        (MARKER.to_owned(), marked_version.clone()),
        "the search row is untouched"
    );
    assert_eq!(
        location_projection_version(pool, key).await,
        location_version,
        "the location projection row is untouched"
    );
    assert_eq!(
        temporal_instances(client, &id, "location").await.len(),
        1,
        "only the creation recorded the location"
    );

    // Clause 5.6.3: a new attribute rewrites the search row (its attribute
    // names change), and the row keeps the geometry it held rather than a
    // re-projection of the location.
    write(
        client,
        Method::POST,
        &attrs,
        &json!({"colour": {"type": "Property", "value": "red"}, "@context": core_context()}),
        StatusCode::NO_CONTENT,
    )
    .await;
    let (location, version) = search_row(pool, key).await;
    assert!(version != marked_version, "a new attribute name rewrites the search row");
    assert_eq!(location, MARKER, "the rewritten row keeps the stored geometry");

    // A write that changes the location projects it.
    write(
        client,
        Method::PATCH,
        &attrs,
        &json!({"location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [3.0, 4.0]}}, "@context": core_context()}),
        StatusCode::NO_CONTENT,
    )
    .await;
    assert_eq!(search_row(pool, key).await.0, "POINT(3 4)", "a changed location is projected");

    scope.finish().await.expect("cleanup failed");
}
