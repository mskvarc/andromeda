//! A near geoquery reads a registration location across the antimeridian and apart from an antipodal corner.

use andromeda_test_suite::{broker, config::SuiteConfig, geometry_round_trip::rectangle, namespace::Namespace, problem::answered};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::RegisterContextSource},
    model::{
        RawJson,
        geometry::{Position, Shape},
    },
    query::{geoquery, geoquery::GeoQuery},
    registration::{CSourceRegistration, RegistrationId, RegistrationInfo},
};
use std::collections::HashSet;

/// Pins: for registrations of the type Zone whose location is the polygon (170,-10), (-170,-10), (-170,10), (170,10)
/// for the first and the square (-10,-10) to (10,10) for the second, Query Context Source Registrations with
/// near;maxDistance==25000 around (180,0) answers the first, with near;maxDistance==1000 around the square (0,0) to
/// (10,10) the second, with near;minDistance==1000 around it both and with near;maxDistance==1000 around the square
/// (170,0) to (180,10) the first; the first polygon is the one 20 degrees wide around the antimeridian, each square
/// lies 16 890 km from the polygon one of whose corners is the antipode of one of its own, and a location bounds
/// where a source's Entities are, so any of them may lie beyond a minimum distance
/// Clause: 5.10.2.4, 4.10
#[gtest]
#[tokio::test]
async fn n_csr_qry_9099_a_near_geoquery_reads_a_registration_location_across_the_antimeridian_and_apart_from_an_antipodal_corner() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let zone_type = namespace.entity_type("Zone")?;
    let antimeridian = namespace.registration_id("antimeridian")?;
    let centre = namespace.registration_id("centre")?;
    let locations = [
        (
            &antimeridian,
            r#"{"type": "Polygon", "coordinates": [[[170, -10], [-170, -10], [-170, 10], [170, 10], [170, -10]]]}"#,
        ),
        (
            &centre,
            r#"{"type": "Polygon", "coordinates": [[[-10, -10], [10, -10], [10, 10], [-10, 10], [-10, -10]]]}"#,
        ),
    ];
    for (id, location) in locations {
        let registration = CSourceRegistration::builder(config.unreachable_origin().as_str())
            .id(id.clone())
            .information([RegistrationInfo::builder().entities([zone_type.as_str()]).build()?])
            .build()?;
        // The model refuses it (a registration location), so `location` is written as text.
        let location: RawJson = location.parse()?;
        let document = UncheckedDocument::try_from(registration.render())?.set_member("location", &location);
        let request = broker.unchecked(&RegisterContextSource { registration: &registration })?.document(&document);
        let answer = broker.send_unchecked(request).await;
        expect_that!(&answer, answered(StatusCode::CREATED));
    }
    let cases: [(&str, GeoQuery, &[&RegistrationId]); 4] = [
        (
            "near;maxDistance==25000 around (180,0)",
            geoquery::near_max(25_000, Shape::Point(Position::try_new(180.0, 0.0)?)).build()?,
            &[&antimeridian],
        ),
        (
            "near;maxDistance==1000 around the square (0,0) to (10,10)",
            geoquery::near_max(1000, rectangle(0.0, 0.0, 10.0, 10.0)?).build()?,
            &[&centre],
        ),
        (
            "near;minDistance==1000 around the square (0,0) to (10,10)",
            geoquery::near_min(1000, rectangle(0.0, 0.0, 10.0, 10.0)?).build()?,
            &[&antimeridian, &centre],
        ),
        (
            "near;maxDistance==1000 around the square (170,0) to (180,10)",
            geoquery::near_max(1000, rectangle(170.0, 0.0, 180.0, 10.0)?).build()?,
            &[&antimeridian],
        ),
    ];
    for (label, geo_q, selects) in cases {
        let page = broker
            .with_accept(Accept::any())
            .registrations()
            .query()
            .geo_q(geo_q)
            .entity_type(&zone_type)
            .send()
            .await?;
        let answered: HashSet<RegistrationId> = page.items().iter().map(|registration| registration.id().clone()).collect();
        let expected: HashSet<RegistrationId> = selects.iter().map(|id| (*id).clone()).collect();
        expect_that!(answered, eq(&expected), "Query Context Source Registrations with {label}");
    }
    for id in [&antimeridian, &centre] {
        broker.registrations().delete(id).send().await?;
    }
    Ok(())
}
