//! A near geoquery reaches a long edge of a registration location where it bows.

use andromeda_test_suite::{broker, config::SuiteConfig, identity_matcher::has_id, namespace::Namespace, nth::nth, problem::answered};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::RegisterContextSource},
    model::{
        RawJson,
        geometry::{Position, Shape},
    },
    query::geoquery,
    registration::{CSourceRegistration, RegistrationInfo},
};

/// Pins: Query Context Source Registrations near;maxDistance==1000 around (0,63.43) answers the registration whose location is the `LineString` (-30,60) to (30,60), and near;maxDistance==500 does not; the great circle through (-30,60) and (30,60) bows north to 63.43, so (0,63.43) is 551.64 m from the edge although it lies 380 km north of both vertices
/// Clause: 5.10.2.4, 4.10
#[gtest]
#[tokio::test]
async fn n_csr_qry_9090_a_near_geoquery_reaches_a_long_edge_of_a_registration_location_where_it_bows() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let registration_route_id = namespace.registration_id("route")?;
    let route_type = namespace.entity_type("Route")?;
    let registration_route = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_route_id.clone())
        .information([RegistrationInfo::builder().entities([route_type.as_str()]).build()?])
        .build()?;
    // The model refuses it (a registration location), so `location` is written as text.
    let location: RawJson = r#"{"type": "LineString", "coordinates": [[-30.0, 60.0], [30.0, 60.0]]}"#.parse()?;
    let document = UncheckedDocument::try_from(registration_route.render())?.set_member("location", &location);
    let request = broker
        .unchecked(&RegisterContextSource {
            registration: &registration_route,
        })?
        .document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let page = broker
        .with_accept(Accept::any())
        .registrations()
        .query()
        .geo_q(geoquery::near_max(1000, Shape::Point(Position::try_new(0.0, 63.43)?)))
        .entity_type(&route_type)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&registration_route_id)));
    let page_2 = broker
        .with_accept(Accept::any())
        .registrations()
        .query()
        .geo_q(geoquery::near_max(500, Shape::Point(Position::try_new(0.0, 63.43)?)))
        .entity_type(&route_type)
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(0)));
    broker.registrations().delete(&registration_route_id).send().await?;
    Ok(())
}
