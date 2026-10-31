//! Ids of 128 512 and 1024 octets round trip with their attributes.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::{has_attribute, member_count},
    exact_length::padded,
    namespace::Namespace,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::{Accept, Location},
    model::{Entity, EntityId, NgsiLdMediaType},
};

/// Pins: Entities whose ids are 128, 512 and 1,024 octets long, holding 1, 10 and 100 attributes, are created and
/// retrieved by the Location they are answered with, each reading back its id and every attribute
/// Clause: 5.6.1, 5.7.1, 4.6.2
/// Recorded: ETSI GS CIM 009 sets no length on an identifier; Andromeda bounds every identifier it stores at 1,024
/// octets, the value as stored, which for a term is its expansion
#[gtest]
#[tokio::test]
async fn a_lim_idn_9008_ids_of_128_512_and_1024_octets_round_trip_with_their_attributes() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_type = namespace.entity_type("Building")?;
    for (width, attributes) in [(128_usize, 1_i32), (512, 10), (1024, 100)] {
        let id = padded(&namespace.urn_text("Building", format!("width{width}-")), width)?.parse::<EntityId>()?;
        let building = (0..attributes)
            .fold(Entity::builder().id(id.clone()).entity_type(building_type.clone()), |builder, index| {
                builder.property(format!("attr{index}"), index)
            })
            .build()?;
        let created = broker.with_responses().entities().create(&building).send().await?;
        expect_that!(
            created.head.typed::<Location>().valid().map(|location| location.as_iri_ref().as_str()),
            some(eq(format!("/ngsi-ld/v1/entities/{id}").as_str()))
        );
        let retrieved = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).entities().retrieve(&id).send().await?;
        let entity = retrieved.body();
        expect_that!(entity.id, eq(&id));
        expect_that!(entity.id.to_string().len(), eq(width));
        expect_that!(member_count(entity), eq(usize::try_from(attributes)? + 2));
        expect_that!(entity, has_attribute("attr0", any_attribute().value(0)));
        expect_that!(entity, has_attribute(&format!("attr{}", attributes - 1), any_attribute().value(attributes - 1)));
    }
    Ok(())
}
