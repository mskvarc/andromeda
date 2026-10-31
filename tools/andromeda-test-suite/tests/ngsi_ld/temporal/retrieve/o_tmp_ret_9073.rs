//! A history ordinate reads back as its nearest double.

use andromeda_test_suite::{attribute_matcher::any_attribute, broker, evolution_matcher::has_evolution, namespace::Namespace, nth::nth, problem::answered};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        EntityId,
        GeoProperty,
        NgsiLdMediaType,
        evolution::TemporalEntity,
        geometry::{Position, Shape},
    },
};

/// Pins: A temporal Tree whose location instance is the Point `[13.123456789012345678, 52.123456789012345678]`
/// reads that instance back as `[13.123456789012346,52.123456789012344]`, the ordinates the Core API returns for it
/// Clause: 4.7.2, 5.7.3
/// Recorded: clause 4.7.2 leaves the precision of a position to the implementation; Andromeda holds every ordinate
/// as the double nearest to it, in the history as in the current state, and writes it in the shortest spelling of
/// that double
#[gtest]
#[tokio::test]
async fn o_tmp_ret_9073_a_history_ordinate_reads_back_as_its_nearest_double() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let tree_type = namespace.entity_type("Tree")?;
    let ngsi_ld_tree_id = format!("urn:ngsi-ld:Tree:{namespace}").parse::<EntityId>()?;
    let entity_evolution = TemporalEntity::builder()
        .id(format!("urn:ngsi-ld:Tree:{namespace}").parse::<EntityId>()?)
        .entity_type(tree_type)
        .geo_property(
            "location",
            GeoProperty::builder(Shape::Point(Position::try_from_numbers(
                "13.123456789012345678".parse()?,
                "52.123456789012345678".parse()?,
            )?))
            .observed_at("2020-10-01T12:00:00Z".parse()?),
        )
        .build()?;
    let answer = broker.with_responses().temporal().create_or_update(&entity_evolution).send().await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let received = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&ngsi_ld_tree_id)
        .send()
        .await?;
    expect_that!(
        String::from_utf8_lossy(&received.body),
        contains_substring("\"coordinates\":[13.123456789012346,52.123456789012344]")
    );
    let retrieved = &received.outcome;
    let evolution = retrieved.body();
    expect_that!(
        evolution,
        has_evolution("location", all!(len(eq(1)), nth(0, any_attribute().value_member(["type"], "Point"))))
    );
    let received_2 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&ngsi_ld_tree_id)
        .send()
        .await?;
    expect_that!(
        String::from_utf8_lossy(&received_2.body),
        contains_substring("\"coordinates\":[13.123456789012346,52.123456789012344]")
    );
    Ok(())
}
