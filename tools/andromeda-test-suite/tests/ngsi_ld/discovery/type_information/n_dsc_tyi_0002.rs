//! Is not found once every entity is deleted.

use andromeda_test_suite::{
    body_json::{json_absent, json_exists, json_includes},
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
    model::{Entity, ErrorType},
};

/// Pins: deleting an entity lowers its type's `entityCount`, and once every entity of the type is deleted the type
/// information is `ResourceNotFound` and neither the type nor its attributes are listed any more
/// Clause: 5.7.7.4, 5.2.26, 6.26.3.1, 5.7.5.4, 5.7.8.4
#[gtest]
#[tokio::test]
async fn n_dsc_tyi_0002_is_not_found_once_every_entity_is_deleted() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_2_id = namespace.urn("Building", 2)?;
    let building_type = namespace.entity_type("Building")?;
    let height = namespace.attribute("height")?;
    let owner = namespace.attribute("owner")?;
    let entities = [
        Entity::builder()
            .id(building_id.clone())
            .entity_type(building_type.clone())
            .property(namespace.attribute("height")?, 1)
            .build()?,
        Entity::builder()
            .id(building_2_id.clone())
            .entity_type(building_type.clone())
            .property(namespace.attribute("height")?, 2)
            .relationship(namespace.attribute("owner")?, building_id.clone())
            .build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    broker.entities().delete(&building_id).send().await?.done()?;
    let information = broker.with_accept(Accept::any()).entity_types().retrieve(&building_type).send().await?;
    expect_that!(information.entity_count, eq(1));
    expect_that!(information.attribute_details, len(eq(2)));
    expect_that!(attribute_names(&information), unordered_elements_are![eq(&height), eq(&owner)]);
    broker.entities().delete(&building_2_id).send().await?.done()?;
    let answer = broker
        .with_responses()
        .with_accept(Accept::any())
        .entity_types()
        .retrieve(&building_type)
        .send()
        .await;
    expect_that!(&answer, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    let client = broker.with_accept(Accept::any());
    let types = client.with_heads().entity_types().list().send().await?;
    expect_that!(types.status, eq(StatusCode::OK));
    expect_that!(types.outcome.type_list, not(contains(eq(&building_type))));
    // Sibling tests leave the reserved `ngsildproof` attribute in the tenant, which the typed list refuses, so the catalog is read as received.
    let attributes = client.send_unchecked(client.unchecked(&RetrieveAttributes::builder().build())?).await?;
    expect_that!(attributes.status(), eq(StatusCode::OK));
    expect_that!(attributes.body(), json_exists("$.attributeList"));
    expect_that!(attributes.body(), not(json_includes("$.attributeList", height.to_string())));
    expect_that!(attributes.body(), not(json_includes("$.attributeList", owner.to_string())));
    let details = client.send_unchecked(client.unchecked(&RetrieveEntityTypeDetails::builder().build())?).await?;
    expect_that!(details.status(), eq(StatusCode::OK));
    expect_that!(details.body(), json_absent(&format!("$[?@.typeName == '{building_type}']")));
    Ok(())
}
