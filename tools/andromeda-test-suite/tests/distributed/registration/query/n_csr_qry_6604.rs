//! Observed at matches the observation interval and management properties the management interval.

use andromeda_test_suite::{broker, cleanup::Cleanup, config::SuiteConfig, identity_matcher::has_id, namespace::Namespace};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::NgsiLdMediaType,
    query::temporal_query::{self, TemporalProperty},
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo, TimeInterval},
};

/// Pins: Query Context Source Registrations by type and temporal queries answer 200 with the registration whose
/// relevant interval holds a `timeAt` for before and after or overlaps the window for between, and without it when
/// the instant lies outside, the interval is of the other kind, or the window is disjoint
/// Clause: 5.10.2.4, 5.12
#[gtest]
#[tokio::test]
async fn n_csr_qry_6604_observed_at_matches_the_observation_interval_and_management_properties_the_management_interval() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_2_id = namespace.registration_id(2)?;
    let registration = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .observation_interval(TimeInterval::between("2020-01-01T00:00:00Z".parse()?, "2021-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let registration_2 = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_2_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .management_interval(TimeInterval::between("2020-01-01T00:00:00Z".parse()?, "2021-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.registrations().register(&registration_2).send().await?;
    let _registration_2_cleanup = Cleanup::of(&broker).registration(&registration_2_id);
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .query()
        .temporal_q(temporal_query::after("2020-06-01T00:00:00Z".parse()?).time_property(TemporalProperty::ObservedAt))
        .entity_type(&vehicle_type)
        .send()
        .await?;
    expect_that!(page.items(), contains(has_id(&registration_id)));
    expect_that!(page.items(), not(contains(has_id(&registration_2_id))));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .query()
        .temporal_q(temporal_query::after("2022-06-01T00:00:00Z".parse()?).time_property(TemporalProperty::ObservedAt))
        .entity_type(&vehicle_type)
        .send()
        .await?;
    expect_that!(page_2.items(), not(contains(has_id(&registration_id))));
    expect_that!(page_2.items(), not(contains(has_id(&registration_2_id))));
    let page_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .query()
        .temporal_q(temporal_query::after("2020-06-01T00:00:00Z".parse()?).time_property(TemporalProperty::CreatedAt))
        .entity_type(&vehicle_type)
        .send()
        .await?;
    expect_that!(page_3.items(), contains(has_id(&registration_2_id)));
    expect_that!(page_3.items(), not(contains(has_id(&registration_id))));
    let page_4 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .query()
        .temporal_q(temporal_query::before("2020-06-01T00:00:00Z".parse()?).time_property(TemporalProperty::ObservedAt))
        .entity_type(&vehicle_type)
        .send()
        .await?;
    expect_that!(page_4.items(), contains(has_id(&registration_id)));
    expect_that!(page_4.items(), not(contains(has_id(&registration_2_id))));
    let page_5 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .query()
        .temporal_q(temporal_query::before("2019-01-01T00:00:00Z".parse()?).time_property(TemporalProperty::ObservedAt))
        .entity_type(&vehicle_type)
        .send()
        .await?;
    expect_that!(page_5.items(), not(contains(has_id(&registration_id))));
    expect_that!(page_5.items(), not(contains(has_id(&registration_2_id))));
    let page_6 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .query()
        .temporal_q(temporal_query::between("2020-06-01T00:00:00Z".parse()?, "2020-09-01T00:00:00Z".parse()?).time_property(TemporalProperty::ObservedAt))
        .entity_type(&vehicle_type)
        .send()
        .await?;
    expect_that!(page_6.items(), contains(has_id(&registration_id)));
    expect_that!(page_6.items(), not(contains(has_id(&registration_2_id))));
    let page_7 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .query()
        .temporal_q(temporal_query::between("2018-01-01T00:00:00Z".parse()?, "2019-01-01T00:00:00Z".parse()?).time_property(TemporalProperty::ObservedAt))
        .entity_type(&vehicle_type)
        .send()
        .await?;
    expect_that!(page_7.items(), not(contains(has_id(&registration_id))));
    expect_that!(page_7.items(), not(contains(has_id(&registration_2_id))));
    Ok(())
}
