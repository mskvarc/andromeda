//! A csf compares a Context Source Property by its JSON type alone.

use andromeda_test_suite::{broker, cleanup::Cleanup, config::SuiteConfig, identity_matcher::has_id, namespace::Namespace, nth::nth};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Input, NgsiLdMediaType, Value},
    query::q,
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
};

/// Pins: over two registrations whose Context Source Property `reading` is the JSON string "4" and the JSON number 4,
/// Query Context Source Registrations with csf `reading==4` answers only the number one and with `reading=="4"` only
/// the string one
/// Clause: 4.9, 5.10.2.4, 5.2.9
#[gtest]
#[tokio::test]
async fn n_csr_qry_9013_a_csf_compares_a_context_source_property_by_its_json_type() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let sensor_type = namespace.entity_type("Sensor")?;
    let registration_string_id = namespace.registration_id("string")?;
    let registration_number_id = namespace.registration_id("number")?;
    let registration = |id, reading: Value| -> Result<CSourceRegistration<Input>> {
        Ok(CSourceRegistration::builder(config.unreachable_origin().as_str())
            .id(id)
            .information([RegistrationInfo::builder()
                .entities([EntityInfo::new(sensor_type.clone()).with_id_pattern(namespace.id_pattern("Sensor")?)])
                .build()?])
            .property("reading", reading)
            .build()?)
    };
    broker
        .registrations()
        .register(&registration(registration_string_id.clone(), Value::from("4"))?)
        .send()
        .await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_string_cleanup = Cleanup::of(&broker).registration(&registration_string_id);
    broker
        .registrations()
        .register(&registration(registration_number_id.clone(), Value::from(4))?)
        .send()
        .await?;
    let _registration_number_cleanup = Cleanup::of(&broker).registration(&registration_number_id);
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .query()
        .entity_type(&sensor_type)
        .csf(q::attr("reading").eq(4))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&registration_number_id)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .query()
        .entity_type(&sensor_type)
        .csf(q::attr("reading").eq("4"))
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(1)));
    expect_that!(page_2.items(), nth(0, has_id(&registration_string_id)));
    Ok(())
}
