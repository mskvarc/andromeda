//! An offset at or past the end answers an empty array with only prev.

use andromeda_test_suite::{broker, cleanup::Cleanup, config::SuiteConfig, namespace::Namespace};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::ResultsCount},
    model::NgsiLdMediaType,
    query::IdPattern,
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
};

/// Pins: Query Context Source Registrations with limit 2 and count true over two matching registrations and an offset
/// of 2 or 3 answers 200 with an empty array, `NGSILD-Results-Count` 2 and a prev pointer alone
/// Clause: 5.5.9.2, 6.3.10, Table 6.3.10-2, 6.3.13
/// Recorded: Table 6.3.10-2 makes the too-large offset a "should" and its rule sets the offset to the size of the result
/// set, which selects nothing; Andromeda answers 200 with an empty array
#[gtest]
#[tokio::test]
async fn o_csr_qry_9020_an_offset_at_or_past_the_end_answers_an_empty_array_with_only_prev() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let building_type = namespace.entity_type("Building")?;
    let mut registration_cleanup = Cleanup::of(&broker);
    for n in 0..2 {
        let registration_id = namespace.registration_id(format!("{n:04}"))?;
        let registration = CSourceRegistration::builder(config.unreachable_origin().as_str())
            .id(registration_id.clone())
            .information([RegistrationInfo::builder()
                .entities([EntityInfo::new(building_type.clone()).with_id_pattern(format!("urn:ngsi-ld:Building:{namespace}-.*").parse::<IdPattern>()?)])
                .property_names(["name"])
                .build()?])
            .build()?;
        broker.registrations().register(&registration).send().await?;
        // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
        // cooldown that opens there holds back a later test's registration naming the same endpoint.
        registration_cleanup = registration_cleanup.registration(&registration_id);
    }
    let _registration_cleanup = registration_cleanup;
    for offset in [2, 3] {
        let page = broker
            .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
            .registrations()
            .query()
            .entity_type(&building_type)
            .limit(2)
            .offset(offset)
            .count(ResultsCount::Include)
            .send()
            .await?;
        expect_that!(page.items(), is_empty());
        expect_that!(page.total(), some(eq(2)));
        expect_that!(page.prev(), some(anything()));
        expect_that!(page.next(), none());
        expect_that!(page.first(), none());
        expect_that!(page.last(), none());
    }
    Ok(())
}
