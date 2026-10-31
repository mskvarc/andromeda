//! A geometry foreign member holding a space reads back as written.

use andromeda_test_suite::{attribute_matcher::geo_property, broker, entity_matcher::has_attribute, namespace::Namespace, problem::answered};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::CreateEntity},
    model::{Entity, Lossless, NgsiLdMediaType, RawJson, Value},
};

/// Pins: A `GeoProperty` Point carrying the foreign member "plot name" reads back with the member as written
/// Clause: 4.6.4, 4.7, RFC 7946 section 6.1
#[gtest]
#[tokio::test]
async fn n_ent_ret_9031_a_geometry_foreign_member_holding_a_space_reads_back_as_written() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let shop_foreign_member_id = namespace.urn("Shop", "foreign-member")?;
    let shop_type = namespace.entity_type("Shop")?;
    let shop_foreign_member = Entity::builder().id(shop_foreign_member_id.clone()).entity_type(shop_type).build()?;
    // The model refuses it (a geometry with foreign members), so `p` is written as text.
    let p: RawJson = r#"{"type": "GeoProperty", "value": {"type": "Point", "coordinates": [1.5, 2.5], "plot name": "7"}}"#.parse()?;
    let document = UncheckedDocument::from_entity(&shop_foreign_member, Lossless::Normalized).set_member("p", &p);
    let request = broker
        .unchecked(&CreateEntity::builder().entity(&shop_foreign_member).build())?
        .document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&shop_foreign_member_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(
        entity,
        has_attribute(
            "p",
            geo_property()
                .value_member(["type"], "Point")
                .value_member(["coordinates", "0"], Value::Number("1.5".parse()?))
                .value_member(["coordinates", "1"], Value::Number("2.5".parse()?))
                .value_member(["plot name"], "7")
        )
    );
    Ok(())
}
