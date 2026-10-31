//! A between window matches an interval it overlaps and not a disjoint one.

use andromeda_test_suite::{broker, cleanup::Cleanup, context_source::ContextSource, identity_matcher::has_id, namespace::Namespace, nth::nth};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    query::temporal_query::{self, TemporalProperty},
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo, TimeInterval},
};

/// Pins: Query Context Source Registrations with timerel between over a window inside a `managementInterval`
/// (timeproperty `modifiedAt`) or an `observationInterval` (timeproperty `observedAt`) answers that registration,
/// and over a window after the interval answers none
/// Clause: 5.10.2.4, 5.2.9, 5.2.11
#[gtest]
#[tokio::test]
async fn n_csr_qry_6002_a_between_window_matches_an_interval_it_overlaps_and_not_a_disjoint_one() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_2_id = namespace.registration_id(2)?;
    let vehicle_2_id = namespace.urn("Vehicle", 2)?;
    let registration = CSourceRegistration::builder(source.endpoint("1")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_id.clone())])
            .build()?])
        .management_interval(TimeInterval::between("2020-01-01T00:00:00Z".parse()?, "2020-12-31T00:00:00Z".parse()?))
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let registration_2 = CSourceRegistration::builder(source.endpoint("2")?)
        .id(registration_2_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id(vehicle_2_id.clone())])
            .build()?])
        .observation_interval(TimeInterval::between("2020-01-01T00:00:00Z".parse()?, "2020-12-31T00:00:00Z".parse()?))
        .build()?;
    broker.registrations().register(&registration_2).send().await?;
    let _registration_2_cleanup = Cleanup::of(&broker).registration(&registration_2_id);
    let page = broker
        .with_accept(Accept::any())
        .registrations()
        .query()
        .temporal_q(temporal_query::between("2020-06-01T00:00:00Z".parse()?, "2020-06-30T00:00:00Z".parse()?).time_property(TemporalProperty::ModifiedAt))
        .entity_type(&vehicle_type)
        .ids(vec![vehicle_id.clone()])
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&registration_id)));
    let page_2 = broker
        .with_accept(Accept::any())
        .registrations()
        .query()
        .temporal_q(temporal_query::between("2025-01-01T00:00:00Z".parse()?, "2025-12-31T00:00:00Z".parse()?).time_property(TemporalProperty::ModifiedAt))
        .entity_type(&vehicle_type)
        .ids(vec![vehicle_id.clone()])
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(0)));
    let page_3 = broker
        .with_accept(Accept::any())
        .registrations()
        .query()
        .temporal_q(temporal_query::between("2020-06-01T00:00:00Z".parse()?, "2020-06-30T00:00:00Z".parse()?).time_property(TemporalProperty::ObservedAt))
        .entity_type(&vehicle_type)
        .ids(vec![vehicle_2_id.clone()])
        .send()
        .await?;
    expect_that!(page_3.items(), len(eq(1)));
    expect_that!(page_3.items(), nth(0, has_id(&registration_2_id)));
    let page_4 = broker
        .with_accept(Accept::any())
        .registrations()
        .query()
        .temporal_q(temporal_query::between("2025-01-01T00:00:00Z".parse()?, "2025-12-31T00:00:00Z".parse()?).time_property(TemporalProperty::ObservedAt))
        .entity_type(&vehicle_type)
        .ids(vec![vehicle_2_id.clone()])
        .send()
        .await?;
    expect_that!(page_4.items(), len(eq(0)));
    Ok(())
}
