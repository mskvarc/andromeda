//! A downloaded context carrying other members resolves through its subtree.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Done,
    remote_context::RemoteContexts,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{ContextDelivery, headers::Accept},
    model::{Context, Entity, EntityTypeName, NgsiLdMediaType, OneOrMany},
};

/// Pins: Create Entity naming a remote @context whose document carries an id and an unrelated member beside its
/// @context answers 201, and the entity reads back with the type and name the subtree defines
/// Clause: 5.13.2.3, 5.13.1, 5.6.1.4
#[gtest]
#[tokio::test]
async fn n_ctx_exp_9014_a_downloaded_context_carrying_other_members_resolves_through_its_subtree() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let contexts = RemoteContexts::start().await?;
    let body_context = broker
        .with_context(Context::Multiple(vec![contexts.reference("ctx/extras.jsonld")?]))?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building_type_2 = format!("https://test.andromeda/{namespace}/Building").parse::<EntityTypeName>()?;
    contexts
        .serve(
            "ctx/extras.jsonld",
            &format!(
                r#"{{
      "@context": {{
        "Building_{namespace}": "https://test.andromeda/{namespace}/Building",
        "name": "https://test.andromeda/{namespace}/name"
      }},
      "id": "urn:ngsi-ld:Context:{namespace}-from-the-origin-server",
      "unrelated": [
        1,
        2,
        3
      ]
    }}"#
            ),
            Some("application/ld+json"),
        )
        .await?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type)
        .property("name", "Eiffel Tower")
        .build()?;
    body_context.entities().create(&building).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity.entity_type, eq(&OneOrMany::one(building_type_2)));
    expect_that!(
        entity,
        has_attribute(&format!("https://test.andromeda/{namespace}/name"), any_attribute().value("Eiffel Tower"))
    );
    Ok(())
}
