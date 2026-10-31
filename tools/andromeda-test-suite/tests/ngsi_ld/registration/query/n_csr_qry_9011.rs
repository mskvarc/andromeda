//! A self intersecting registration location reads back as written and is judged repaired.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    config::SuiteConfig,
    geometry_matcher::geometry,
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    problem::answered,
};
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

/// Pins: A registration whose location is the self-intersecting Polygon `[[[0,0]`,`[2,2]`,`[2,0]`,`[0,2]`,`[0,0]`]]
/// reads that location back as that Polygon, and a discovery by type and intersects answers it at `[0.3,1.0]`,
/// inside a lobe, and not at `[1.0,0.3]`, between the lobes
/// Clause: 4.7.1, 4.7.2, 5.10.2.4
/// Recorded: clause 4.7.1 makes the `GeoProperty` value the value read back, and clause 4.7.2 accepts every geometry
/// meeting IETF RFC 7946, whose section 3.1.6 sets no simplicity rule on a ring; Andromeda keeps a self-intersecting
/// Polygon as written and judges a geoquery against its `ST_MakeValid` repair, whose lobes are the triangles
/// (0,0)-(1,1)-(0,2) and (1,1)-(2,2)-(2,0); clause 5.10.2.4 matches an intersects geoquery against a registration
/// location the location intersects
#[gtest]
#[tokio::test]
async fn n_csr_qry_9011_a_self_intersecting_registration_location_reads_back_as_written_and_is_judged_repaired() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let registration_bow_tie_id = namespace.registration_id("bow-tie")?;
    let building_type = namespace.entity_type("Building")?;
    let registration_bow_tie = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_bow_tie_id.clone())
        .information([RegistrationInfo::builder().entities([building_type.as_str()]).build()?])
        .build()?;
    // The model refuses it (a registration location), so `location` is written as text.
    let location: RawJson = r#"{"type": "Polygon", "coordinates": [[[0, 0], [2, 2], [2, 0], [0, 2], [0, 0]]]}"#.parse()?;
    let document = UncheckedDocument::try_from(registration_bow_tie.render())?.set_member("location", &location);
    let request = broker
        .unchecked(&RegisterContextSource {
            registration: &registration_bow_tie,
        })?
        .document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_bow_tie_cleanup = Cleanup::of(&broker).registration(&registration_bow_tie_id);
    let registration = broker
        .with_accept(Accept::any())
        .registrations()
        .retrieve(&registration_bow_tie_id)
        .send()
        .await?;
    expect_that!(
        registration.location(),
        some(
            geometry()
                .of_type("Polygon")
                .length([], 1)
                .length([0], 5)
                .length([0, 0], 2)
                .ordinate([0, 0, 0], 0)
                .ordinate([0, 0, 1], 0)
                .length([0, 1], 2)
                .ordinate([0, 1, 0], 2)
                .ordinate([0, 1, 1], 2)
                .length([0, 2], 2)
                .ordinate([0, 2, 0], 2)
                .ordinate([0, 2, 1], 0)
                .length([0, 3], 2)
                .ordinate([0, 3, 0], 0)
                .ordinate([0, 3, 1], 2)
                .length([0, 4], 2)
                .ordinate([0, 4, 0], 0)
                .ordinate([0, 4, 1], 0)
        )
    );
    let page = broker
        .with_accept(Accept::any())
        .registrations()
        .query()
        .geo_q(geoquery::intersects(Shape::Point(Position::try_new(0.3, 1.0)?)))
        .entity_type(&building_type)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&registration_bow_tie_id)));
    let page_2 = broker
        .with_accept(Accept::any())
        .registrations()
        .query()
        .geo_q(geoquery::intersects(Shape::Point(Position::try_new(1.0, 0.3)?)))
        .entity_type(&building_type)
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(0)));
    broker.registrations().delete(&registration_bow_tie_id).send().await?;
    Ok(())
}
