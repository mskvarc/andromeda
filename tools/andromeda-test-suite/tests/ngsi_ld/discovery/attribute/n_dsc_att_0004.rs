//! An entity scope is never listed.

use andromeda_test_suite::{body_json::json_includes, broker, namespace::Namespace, outcome::Done, type_information::attribute_names};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::RetrieveAttributes},
    model::Entity,
};

/// Pins: the scope of an entity is an entity member, not an attribute: Retrieve Available Attributes lists the
/// entity's attribute and never scope, and the entity type information names the attribute alone
/// Clause: 5.7.8.4, 5.7.7.4, 4.18, 5.2.4
#[gtest]
#[tokio::test]
async fn n_dsc_att_0004_an_entity_scope_is_never_listed() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let height = namespace.attribute("height")?;
    let building = Entity::builder()
        .id(building_id)
        .entity_type(building_type.clone())
        .scope(format!("/Census/{namespace}").as_str())
        .property(namespace.attribute("height")?, 6)
        .build()?;
    broker.entities().create(&building).send().await?.done()?;
    // Sibling tests leave the reserved `ngsildproof` attribute in the tenant, which the typed list refuses, so the catalog is read as received.
    let client = broker.with_accept(Accept::any());
    let attributes = client.send_unchecked(client.unchecked(&RetrieveAttributes::builder().build())?).await?;
    expect_that!(attributes.status(), eq(StatusCode::OK));
    expect_that!(attributes.body(), json_includes("$.attributeList", height.to_string()));
    expect_that!(attributes.body(), not(json_includes("$.attributeList", "scope")));
    expect_that!(attributes.body(), not(json_includes("$.attributeList", "https://uri.etsi.org/ngsi-ld/scope")));
    let information = broker.with_accept(Accept::any()).entity_types().retrieve(&building_type).send().await?;
    expect_that!(information.entity_count, eq(1));
    expect_that!(information.attribute_details, len(eq(1)));
    expect_that!(attribute_names(&information), elements_are![eq(&height)]);
    Ok(())
}
