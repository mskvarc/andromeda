//! An ld json create reads its terms through the body context.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::{has_attribute, lacks_attribute},
    hosted_context::HostedContext,
    namespace::Namespace,
    outcome::Done,
};
use cefact_units::UnitCode;
use googletest::prelude::*;
use ngsi_ld::{
    http::{
        ContextDelivery,
        headers::{Accept, Location},
    },
    model::{Context, ContextEntry, Entity, EntityTypeName, InlineContext, NgsiLdMediaType, OneOrMany, Property},
};

/// Pins: an application/ld+json Create Entity expands its terms with the @context of its body, so the entity reads
/// back under those terms when that context is linked (as JSON-LD or JSON) and under the full IRIs without it
/// Clause: 6.3.5, 5.6.1, 5.7.1
#[gtest]
#[tokio::test]
async fn n_ctx_exp_0003_an_ld_json_create_reads_its_terms_through_the_body_context() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building_type_2 = format!("https://test.andromeda/{namespace}/Building").parse::<EntityTypeName>()?;
    let context = Context::Single(ContextEntry::Inline(format!(r#"{{"Building_{namespace}": "https://test.andromeda/{namespace}/Building", "name": "https://test.andromeda/{namespace}/name", "subCategory": "https://test.andromeda/{namespace}/subCategory", "almostFull": "https://test.andromeda/{namespace}/almostFull", "airQualityLevel": "https://test.andromeda/{namespace}/airQualityLevel"}}"#).parse::<InlineContext>()?));
    let ctx = HostedContext::add(&broker, &context).await?;
    let body_context = broker
        .with_context(Context::Single(ctx.entry()?))?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
        .property("name", "Eiffel Tower")
        .property("subCategory", "tourism")
        .property("almostFull", false)
        .property(
            "airQualityLevel",
            Property::builder(4).unit_code(UnitCode::C62).observed_at("2020-09-09T16:40:00.000Z".parse()?),
        )
        .build()?;
    let received = body_context.with_responses().entities().create(&building).send().await?;
    received.outcome.done()?;
    expect_that!(
        received.head.typed::<Location>().valid().map(|location| location.as_iri_ref().as_str()),
        some(contains_substring(format!("urn:ngsi-ld:Building:{namespace}-1")))
    );
    let linked = broker.with_context(Context::Single(ctx.entry()?))?;
    let retrieved = linked
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .entities()
        .retrieve(&building_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(entity.id, eq(&building_id));
    expect_that!(entity.entity_type, eq(&OneOrMany::one(building_type.clone())));
    expect_that!(entity, has_attribute("name", any_attribute().value("Eiffel Tower")));
    expect_that!(entity, has_attribute("subCategory", any_attribute().value("tourism")));
    expect_that!(entity, has_attribute("almostFull", any_attribute().value(false)));
    expect_that!(entity, has_attribute("airQualityLevel", any_attribute().value(4).unit_code(UnitCode::C62)));
    let retrieved_2 = linked
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .send()
        .await?;
    let entity_2 = retrieved_2.body();
    expect_that!(entity_2.id, eq(&building_id));
    expect_that!(entity_2.entity_type, eq(&OneOrMany::one(building_type.clone())));
    expect_that!(entity_2, has_attribute("name", any_attribute().value("Eiffel Tower")));
    expect_that!(entity_2, has_attribute("subCategory", any_attribute().value("tourism")));
    expect_that!(entity_2, has_attribute("almostFull", any_attribute().value(false)));
    expect_that!(entity_2, has_attribute("airQualityLevel", any_attribute().value(4).unit_code(UnitCode::C62)));
    let retrieved_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&building_id)
        .send()
        .await?;
    let entity_3 = retrieved_3.body();
    expect_that!(entity_3.id, eq(&building_id));
    expect_that!(entity_3.entity_type, eq(&OneOrMany::one(building_type_2)));
    expect_that!(entity_3, lacks_attribute("name"));
    expect_that!(entity_3, lacks_attribute("subCategory"));
    expect_that!(entity_3, lacks_attribute("almostFull"));
    expect_that!(
        entity_3,
        has_attribute(&format!("https://test.andromeda/{namespace}/name"), any_attribute().value("Eiffel Tower"))
    );
    expect_that!(
        entity_3,
        has_attribute(&format!("https://test.andromeda/{namespace}/subCategory"), any_attribute().value("tourism"))
    );
    expect_that!(
        entity_3,
        has_attribute(&format!("https://test.andromeda/{namespace}/almostFull"), any_attribute().value(false))
    );
    Ok(())
}
