//! A registered and a queried id pattern match anywhere in the identifier unless anchored.

use andromeda_test_suite::{broker, cleanup::Cleanup, context_source::ContextSource, identity_matcher::has_id, namespace::Namespace};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::NgsiLdMediaType,
    query::IdPattern,
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
};

/// Pins: over registrations whose `EntityInfo` holds the `idPattern` `Car:<ns>-1` (r1), the `idPattern`
/// `Car:<ns>-1$` (r2) and the id `urn:ngsi-ld:Car:<ns>-12` (r3), Query Context Source Registrations by type answers
/// r1 and r3 for the id `urn:ngsi-ld:Car:<ns>-12`, r1 and r2 for the id `urn:ngsi-ld:Car:<ns>-1`, all three for the
/// `idPattern` `Car:<ns>-1`, and r1 and r2 for the `idPattern` `Car:<ns>-1$`
/// Clause: 5.2.8, 5.10.2, 5.12
///
/// The pattern is an IEEE 1003.2 regular expression, whose matching searches a string for a matching sequence, and
/// `$` limits the match to the end of the identifier. r1 and r2 answer every queried `idPattern`, since clause 5.12
/// matches an `EntityInfo` whenever both the query and the `EntityInfo` carry a pattern.
#[gtest]
#[tokio::test]
async fn n_csr_qry_9018_a_registered_and_a_queried_id_pattern_match_anywhere_in_the_identifier_unless_anchored() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let car_type = namespace.entity_type("Car")?;
    let car_1_id = namespace.urn("Car", 1)?;
    let car_12_id = namespace.urn("Car", 12)?;
    let registration_r1_id = namespace.registration_id("r1")?;
    let registration_r2_id = namespace.registration_id("r2")?;
    let registration_r3_id = namespace.registration_id("r3")?;
    let registered = [
        (
            &registration_r1_id,
            EntityInfo::new(car_type.clone()).with_id_pattern(format!("Car:{namespace}-1").parse::<IdPattern>()?),
        ),
        (
            &registration_r2_id,
            EntityInfo::new(car_type.clone()).with_id_pattern(format!("Car:{namespace}-1$").parse::<IdPattern>()?),
        ),
        (&registration_r3_id, EntityInfo::new(car_type.clone()).with_id(car_12_id.clone())),
    ];
    let mut cleanups = Vec::new();
    for (registration_id, entity_info) in registered {
        let registration = CSourceRegistration::builder(source.endpoint("1")?)
            .id(registration_id.clone())
            .information([RegistrationInfo::builder().entities([entity_info]).build()?])
            .build()?;
        broker.registrations().register(&registration).send().await?;
        cleanups.push(Cleanup::of(&broker).registration(registration_id));
    }
    let _cleanups = cleanups;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .query()
        .entity_type(&car_type)
        .ids(vec![car_12_id.clone()])
        .send()
        .await?;
    expect_that!(page.items(), len(eq(2)));
    expect_that!(page.items(), contains(has_id(&registration_r1_id)));
    expect_that!(page.items(), contains(has_id(&registration_r3_id)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .query()
        .entity_type(&car_type)
        .ids(vec![car_1_id])
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(2)));
    expect_that!(page_2.items(), contains(has_id(&registration_r1_id)));
    expect_that!(page_2.items(), contains(has_id(&registration_r2_id)));
    let page_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .query()
        .entity_type(&car_type)
        .id_pattern(format!("Car:{namespace}-1").parse::<IdPattern>()?)
        .send()
        .await?;
    expect_that!(page_3.items(), len(eq(3)));
    expect_that!(page_3.items(), contains(has_id(&registration_r1_id)));
    expect_that!(page_3.items(), contains(has_id(&registration_r2_id)));
    expect_that!(page_3.items(), contains(has_id(&registration_r3_id)));
    let page_4 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .query()
        .entity_type(&car_type)
        .id_pattern(format!("Car:{namespace}-1$").parse::<IdPattern>()?)
        .send()
        .await?;
    expect_that!(page_4.items(), len(eq(2)));
    expect_that!(page_4.items(), contains(has_id(&registration_r1_id)));
    expect_that!(page_4.items(), contains(has_id(&registration_r2_id)));
    Ok(())
}
