//! Is not found once its last instance is gone.

use andromeda_test_suite::{
    body_json::{json_count, json_includes},
    broker,
    namespace::Namespace,
    outcome::Done,
    problem::problem,
    type_information::attribute_names,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{
        headers::Accept,
        operation::{RetrieveAttributes, RetrieveEntityTypeDetails},
    },
    model::{
        AttributeKind,
        Entity,
        EntityFragment,
        ErrorType,
        geometry::{Position, Shape},
    },
};

/// Pins: an attribute stops being available when the last entity carrying it loses it, by an attribute deletion or
/// by the deletion of the entity: its information is `ResourceNotFound` and the type details and attribute list
/// drop it; regained by another entity as a Property, it reports that family alone
/// Clause: 5.7.10.4, 5.2.28, 6.28.3.1, 5.7.7.4, 5.7.6.4, 5.7.8.4
#[gtest]
#[tokio::test]
async fn n_dsc_ati_0002_is_not_found_once_its_last_instance_is_gone() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_x_id = namespace.urn("Building", "x")?;
    let building_y_id = namespace.urn("Building", "y")?;
    let building_type = namespace.entity_type("Building")?;
    let height = namespace.attribute("height")?;
    let floors = namespace.attribute("floors")?;
    let position = namespace.attribute("position")?;
    let entities = [
        Entity::builder()
            .id(building_x_id.clone())
            .entity_type(building_type.clone())
            .property(namespace.attribute("height")?, 1)
            .property(namespace.attribute("floors")?, 2)
            .build()?,
        Entity::builder()
            .id(building_y_id.clone())
            .entity_type(building_type.clone())
            .property(namespace.attribute("height")?, 3)
            .property(namespace.attribute("floors")?, 4)
            .geo_property(namespace.attribute("position")?, Shape::Point(Position::try_new(14.5, 46.05)?))
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    broker.entities().delete_attribute(&building_y_id, &position).send().await?.done()?;
    let answer = broker.with_responses().with_accept(Accept::any()).attributes().retrieve(&position).send().await;
    expect_that!(&answer, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    let information = broker.with_accept(Accept::any()).entity_types().retrieve(&building_type).send().await?;
    expect_that!(information.entity_count, eq(2));
    expect_that!(information.attribute_details, len(eq(2)));
    expect_that!(attribute_names(&information), unordered_elements_are![eq(&height), eq(&floors)]);
    // Sibling tests leave the reserved `ngsildproof` attribute in the tenant, which the typed list refuses, so the catalog is read as received.
    let client = broker.with_accept(Accept::any());
    let details = client.send_unchecked(client.unchecked(&RetrieveEntityTypeDetails::builder().build())?).await?;
    expect_that!(details.status(), eq(StatusCode::OK));
    let building = format!("$[?@.typeName == '{building_type}']");
    expect_that!(details.body(), json_count(&format!("{building}.attributeNames"), 2));
    expect_that!(details.body(), json_includes(&format!("{building}.attributeNames"), height.to_string()));
    expect_that!(details.body(), json_includes(&format!("{building}.attributeNames"), floors.to_string()));
    let attributes = client.send_unchecked(client.unchecked(&RetrieveAttributes::builder().build())?).await?;
    expect_that!(attributes.status(), eq(StatusCode::OK));
    expect_that!(attributes.body(), json_includes("$.attributeList", height.to_string()));
    expect_that!(attributes.body(), not(json_includes("$.attributeList", position.to_string())));
    let fragment = EntityFragment::builder().property(namespace.attribute("position")?, 5).build()?;
    broker.entities().append_attributes(&building_x_id, &fragment).send().await?.done()?;
    let regained = broker.with_accept(Accept::any()).attributes().retrieve(&position).send().await?;
    expect_that!(regained.attribute_count, some(eq(1)));
    expect_that!(regained.attribute_types.as_deref(), some(elements_are![eq(&AttributeKind::Property)]));
    expect_that!(regained.type_names.as_deref(), some(elements_are![eq(&building_type)]));
    broker.entities().delete(&building_x_id).send().await?.done()?;
    let answer_2 = broker.with_responses().with_accept(Accept::any()).attributes().retrieve(&position).send().await;
    expect_that!(&answer_2, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    let information_2 = broker.with_accept(Accept::any()).entity_types().retrieve(&building_type).send().await?;
    expect_that!(information_2.entity_count, eq(1));
    Ok(())
}
