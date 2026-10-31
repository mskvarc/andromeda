//! An ordinate reads back as its nearest double while a property number keeps every digit.

use andromeda_test_suite::{attribute_matcher::any_attribute, broker, entity_matcher::has_attribute, namespace::Namespace, problem::answered};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        Entity,
        EntityId,
        NgsiLdMediaType,
        Value,
        geometry::{Position, Shape},
    },
};

/// Pins: A Tree whose location is the Point `[13.123456789012345678, 52.5]` and whose girth Property is
/// 13.123456789012345678 reads back the ordinate as 13.123456789012346 and the girth as 13.123456789012345678, in
/// the normalized and the simplified representation
/// Clause: 4.5.2.2, 4.7.2, 5.7.1
/// Recorded: clause 4.7.2 leaves the precision of a position to the implementation; Andromeda holds every ordinate
/// as the double nearest to it and writes it in the shortest spelling of that double, while a Property number keeps
/// its exact decimal value
#[gtest]
#[tokio::test]
async fn o_ent_cre_9048_an_ordinate_reads_back_as_its_nearest_double_while_a_property_number_keeps_every_digit() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let tree_type = namespace.entity_type("Tree")?;
    let ngsi_ld_tree_id = format!("urn:ngsi-ld:Tree:{namespace}").parse::<EntityId>()?;
    let entity = Entity::builder()
        .id(format!("urn:ngsi-ld:Tree:{namespace}").parse::<EntityId>()?)
        .entity_type(tree_type)
        .property("girth", Value::Number("13.123456789012345678".parse()?))
        .geo_property(
            "location",
            Shape::Point(Position::try_from_numbers("13.123456789012345678".parse()?, "52.5".parse()?)?),
        )
        .build()?;
    let answer = broker.with_responses().entities().create(&entity).send().await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let received = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&ngsi_ld_tree_id)
        .send()
        .await?;
    expect_that!(
        String::from_utf8_lossy(&received.body),
        contains_substring("\"coordinates\":[13.123456789012346,52.5]")
    );
    expect_that!(String::from_utf8_lossy(&received.body), contains_substring("\"value\":13.123456789012345678"));
    let retrieved = &received.outcome;
    let entity_2 = retrieved.body();
    expect_that!(
        entity_2,
        has_attribute("location", any_attribute().value_member(["type"], "Point").length_at(["coordinates"], 2))
    );
    let received_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve_simplified(&ngsi_ld_tree_id)
        .send()
        .await?;
    expect_that!(
        String::from_utf8_lossy(&received_2.body),
        contains_substring("\"coordinates\":[13.123456789012346,52.5]")
    );
    expect_that!(String::from_utf8_lossy(&received_2.body), contains_substring("\"girth\":13.123456789012345678"));
    Ok(())
}
