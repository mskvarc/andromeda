//! A temporal query finds a registration by its observation interval.

use andromeda_test_suite::{broker, cleanup::Cleanup, config::SuiteConfig, identity_matcher::has_id, namespace::Namespace, nth::nth};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::NgsiLdMediaType,
    query::temporal_query,
    registration::{CSourceRegistration, RegistrationInfo, TimeInterval},
};

/// Pins: Query Context Source Registrations over a registration whose `observationInterval` runs from 2026-01-01 to
/// 2026-02-01 answers it for between 2026-01-15 and 2026-03-01, and nothing for between 2026-03-01 and 2026-04-01
/// Clause: 5.10.2.4, 4.11
#[gtest]
#[tokio::test]
async fn n_csr_qry_0033_a_temporal_query_finds_a_registration_by_its_observation_interval() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let registration_id = namespace.registration_id(1)?;
    let building_type = namespace.entity_type("Building")?;
    let registration = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(registration_id.clone())
        .information([RegistrationInfo::builder().entities([building_type.as_str()]).build()?])
        .observation_interval(TimeInterval::between("2026-01-01T00:00:00Z".parse()?, "2026-02-01T00:00:00Z".parse()?))
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .query()
        .temporal_q(temporal_query::between("2026-01-15T00:00:00Z".parse()?, "2026-03-01T00:00:00Z".parse()?))
        .entity_type(&building_type)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&registration_id)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .query()
        .temporal_q(temporal_query::between("2026-03-01T00:00:00Z".parse()?, "2026-04-01T00:00:00Z".parse()?))
        .entity_type(&building_type)
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(0)));
    broker.registrations().delete(&registration_id).send().await?;
    Ok(())
}
