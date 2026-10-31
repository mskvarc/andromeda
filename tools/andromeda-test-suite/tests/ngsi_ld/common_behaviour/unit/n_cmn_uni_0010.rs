//! A packaging common code is admitted and read back.

use andromeda_test_suite::{attribute_matcher::any_attribute, broker, entity_matcher::has_attribute, namespace::Namespace, outcome::Done};
use cefact_units::UnitCode;
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType, Property},
};

/// Pins: Create Entity with `unitCode` XAE, an X-prefixed Recommendation 21 package code that Recommendation 20
/// counts among its common codes, answers 201 and reads the code back
/// Clause: 4.5.2.2, 5.2.5, 5.6.1
/// Recorded: Recommendation 20 Revision 9, pinned by reference `[15]`, admits X followed by a Recommendation 21
/// package code as a common code, so XAE is a conforming `unitCode`
#[gtest]
#[tokio::test]
async fn n_cmn_uni_0010_a_packaging_common_code_is_admitted_and_read_back() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let shipment_id = namespace.urn("Shipment", 1)?;
    let shipment_type = namespace.entity_type("Shipment")?;
    let shipment = Entity::builder()
        .id(shipment_id.clone())
        .entity_type(shipment_type)
        .property("load", Property::builder(3).unit_code(UnitCode::Xae))
        .build()?;
    broker.entities().create(&shipment).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&shipment_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity, has_attribute("load", any_attribute().value(3).unit_code(UnitCode::Xae)));
    Ok(())
}
