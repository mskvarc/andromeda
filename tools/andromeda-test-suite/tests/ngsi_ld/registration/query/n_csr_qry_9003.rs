//! A temporal query reaches a partly overlapping observation interval.

use andromeda_test_suite::{broker, cleanup::Cleanup, config::SuiteConfig, identity_matcher::has_id, namespace::Namespace, nth::nth};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::NgsiLdMediaType,
    query::{
        IdPattern,
        temporal_query::{self, TemporalProperty},
    },
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo, TimeInterval},
};

/// Pins: Query Context Source Registrations over a registration whose `observationInterval` runs from August 2020
/// to August 2021 answers it for before 2021-08-01T22:00 and after 2020-06-01, and not for after 2030 or before
/// 2019
/// Clause: 5.10.2.4, 5.11.2.4
#[gtest]
#[tokio::test]
async fn n_csr_qry_9003_a_temporal_query_reaches_a_partly_overlapping_observation_interval() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let registration_id = namespace.registration_id(1)?;
    let building_type = namespace.entity_type("Building")?;
    let registration = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(building_type.clone()).with_id_pattern(format!("urn:ngsi-ld:.+:{namespace}-.*").parse::<IdPattern>()?)])
            .build()?])
        .observation_interval(TimeInterval::between("2020-08-01T22:07:00Z".parse()?, "2021-08-01T21:07:00Z".parse()?))
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .query()
        .temporal_q(temporal_query::before("2021-08-01T22:00:00Z".parse()?).time_property(TemporalProperty::ObservedAt))
        .entity_type(&building_type)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&registration_id)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .query()
        .temporal_q(temporal_query::after("2020-06-01T22:07:00Z".parse()?).time_property(TemporalProperty::ObservedAt))
        .entity_type(&building_type)
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(1)));
    expect_that!(page_2.items(), nth(0, has_id(&registration_id)));
    let page_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .query()
        .temporal_q(temporal_query::after("2030-01-01T00:00:00Z".parse()?).time_property(TemporalProperty::ObservedAt))
        .entity_type(&building_type)
        .send()
        .await?;
    expect_that!(page_3.items(), len(eq(0)));
    let page_4 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .query()
        .temporal_q(temporal_query::before("2019-01-01T00:00:00Z".parse()?).time_property(TemporalProperty::ObservedAt))
        .entity_type(&building_type)
        .send()
        .await?;
    expect_that!(page_4.items(), len(eq(0)));
    broker.registrations().delete(&registration_id).send().await?;
    Ok(())
}
