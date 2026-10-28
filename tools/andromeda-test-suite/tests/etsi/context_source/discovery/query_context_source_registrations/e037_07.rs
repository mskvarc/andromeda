//! ETSI `037_07`: a query of context source registrations with a geoquery is matched against the registration's
//! `location` (clause 5.10.2).

use crate::{
    suite_keywords::{create_registration, csource_endpoint, delete_registrations},
    suite_payloads::{point, polygon},
};
use andromeda_test_suite::{broker, comparison::matches_document, namespace::Namespace, problem::answered, vocabulary::Vocabulary, wire::json_body};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    query::{geoquery, geoquery::builders::GeoQueryBuilder},
    registration::{CSourceRegistration, RegistrationId, RegistrationInfo},
};
use serde_json::json;

/// Registers the vocabulary and the registration `-1` of every `Building` located at the point -8.521, 41.2, queries
/// the registrations of `Building` with `geo_q`, requires 200 and the registration alone, and deletes it.
async fn queried(geo_q: GeoQueryBuilder) -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Building"]).await?;
    let endpoint = csource_endpoint(&namespace, "my.csource.org:1026").await?;
    let registration = CSourceRegistration::builder(endpoint.as_str())
        .id(namespace.urn_text("ContextSourceRegistration", 1).parse::<RegistrationId>()?)
        .information([RegistrationInfo::builder().entities(["Building"]).build()?])
        .location(point(-8.521, 41.2)?)
        .build()?;
    let _registration = create_registration(&broker, &vocabulary, &registration).await?;

    let received = broker
        .with_context(vocabulary.context())?
        .with_responses()
        .registrations()
        .query()
        .entity_type("Building")
        .geo_q(geo_q)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    expect_that!(
        &body,
        matches_document(json!([{
            "id": namespace.urn_text("ContextSourceRegistration", 1),
            "type": "ContextSourceRegistration",
            "information": [{"entities": [{"type": "Building"}]}],
            "location": {"type": "Point", "coordinates": [-8.521, 41.2]},
            "endpoint": endpoint.as_str(),
        }]))
        .hiding_everywhere("mode")
        .hiding_everywhere("operations")
    );
    delete_registrations(&broker, &namespace, &[1]).await
}

/// Pins: Check that one can query context source registrations. If present, the geoquery is matched against the
/// `GeoProperty` programmatic parameter identified in the geoquery
/// Clause: 5.10.2
/// Corrected: `fix(037_07_01)`: csr location is a bare geojson geometry, not a geoproperty (table 5.2.9-1)
/// Changed: the teardown delete is held to the 204 a deleted registration answers, not only to a status below 400.
#[gtest]
#[tokio::test]
async fn e037_07_01_near_point() -> Result<()> {
    queried(geoquery::near_max(2000, point(-8.503, 41.202)?)).await
}

/// Pins: Check that one can query context source registrations. If present, the geoquery is matched against the
/// `GeoProperty` programmatic parameter identified in the geoquery
/// Clause: 5.10.2
/// Corrected: `fix(037_07_02)`: within-polygon query uses a valid closed geojson polygon (4.10)
/// Corrected: `fix(037_07_01)`: csr location is a bare geojson geometry, not a geoproperty (table 5.2.9-1)
/// Changed: the teardown delete is held to the 204 a deleted registration answers, not only to a status below 400.
#[gtest]
#[tokio::test]
async fn e037_07_02_within_polygon() -> Result<()> {
    let area = polygon(&[
        (-13.503, 47.202),
        (-15.23, 21.37),
        (9.46, 32.57),
        (20.37, 44.653),
        (6.541, 52.961),
        (-13.503, 47.202),
    ])?;
    queried(geoquery::within(area).geoproperty("location")).await
}
