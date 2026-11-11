//! Count reports the total beside a page or alone.

use andromeda_test_suite::{broker, cleanup::Cleanup, config::SuiteConfig, namespace::Namespace};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::ResultsCount},
    model::NgsiLdMediaType,
    registration::{CSourceRegistration, RegistrationInfo},
};

/// Pins: Query Context Source Registrations over three registrations of the queried type with limit 2 and count true
/// answers two registrations and NGSILD-Results-Count 3, and with limit 0 and count true answers an empty array and
/// NGSILD-Results-Count 3
/// Clause: 4.12, 4.13, 6.3.13
#[gtest]
#[tokio::test]
async fn n_csr_qry_9014_count_reports_the_total_beside_a_page_or_alone() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let mut registration_cleanup = Cleanup::of(&broker);
    for suffix in ["0000", "0001", "0002"] {
        let registration_id = namespace.registration_id(suffix)?;
        let registration = CSourceRegistration::builder(config.unreachable_origin().as_str())
            .id(registration_id.clone())
            .information([RegistrationInfo::builder()
                .entities([vehicle_type.as_str()])
                .property_names(["speed"])
                .build()?])
            .build()?;
        broker.registrations().register(&registration).send().await?;
        // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
        // cooldown that opens there holds back a later test's registration naming the same endpoint.
        registration_cleanup = registration_cleanup.registration(&registration_id);
    }
    let _registration_cleanup = registration_cleanup;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .query()
        .entity_type(&vehicle_type)
        .limit(2)
        .count(ResultsCount::Include)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(2)));
    expect_that!(page.total(), some(eq(3)));
    let count_only = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .query()
        .entity_type(&vehicle_type)
        .limit(0)
        .count(ResultsCount::Include)
        .send()
        .await?;
    expect_that!(count_only.items(), is_empty());
    expect_that!(count_only.total(), some(eq(3)));
    Ok(())
}
