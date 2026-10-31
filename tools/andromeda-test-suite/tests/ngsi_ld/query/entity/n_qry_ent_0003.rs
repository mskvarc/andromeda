//! Attrs alone selects and projects the entities holding a listed attribute.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::{attribute_members, has_attribute, member_count},
    hosted_context::HostedContext,
    identity_matcher::has_id,
    namespace::Namespace,
    outcome::Done,
    type_matcher::has_type,
};
use cefact_units::UnitCode;
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{AttributeName, Context, ContextEntry, Entity, InlineContext, NgsiLdMediaType, Property},
};

/// Pins: attrs alone (no type) selects only the entities carrying a listed attribute and reduces each to the listed
/// attributes, here named by their full IRIs, with and without a user @context in the Link header
/// Clause: 5.7.2.4, 5.7.2.5, 6.4.3.2, 5.5.7
/// Tenant: private (its queries select by attributes, q or geometry alone, so the broker reads every entity and
/// forwards to every registration of the tenant)
#[gtest]
#[tokio::test]
async fn n_qry_ent_0003_attrs_alone_selects_and_projects_the_entities_holding_a_listed_attribute() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::private_tenant(&broker::front()?, &namespace).await?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building_2_id = namespace.urn("Building", 2)?;
    let building_3_id = namespace.urn("Building", 3)?;
    let building_4_id = namespace.urn("Building", 4)?;
    let entities = [
        Entity::builder()
            .id(building_id.clone())
            .entity_type(building_type.clone())
            .property("name", "Eiffel Tower")
            .property(namespace.attribute("subCategory")?.to_string().as_str(), "tourism")
            .property(
                namespace.attribute("airQualityLevel")?.to_string().as_str(),
                Property::builder(4).unit_code(UnitCode::C62).observed_at("2020-09-09T16:40:00Z".parse()?),
            )
            .property("almostFull", false)
            .build()?,
        Entity::builder().id(building_2_id).entity_type(building_type.clone()).build()?,
    ];
    broker.entities().batch_create(&entities).send().await?.done()?;
    // attrs is the only selector; the attribute names carry the namespace, so only this file's entities can hold them
    let air_quality = format!("airQualityLevel_{namespace}");
    let sub_category = format!("subCategory_{namespace}");
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .attrs(vec![
            format!("https://uri.etsi.org/ngsi-ld/default-context/{air_quality}").parse::<AttributeName>()?,
            format!("https://uri.etsi.org/ngsi-ld/default-context/{sub_category}").parse::<AttributeName>()?,
        ])
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    let entity = page.items().first().or_fail()?;
    expect_that!(entity, all!(has_id(&building_id), has_type(&building_type)));
    expect_that!(member_count(entity), eq(4));
    expect_that!(attribute_members(entity, &air_quality), some(eq(4)));
    expect_that!(
        entity,
        has_attribute(
            &air_quality,
            any_attribute().value(4).unit_code(UnitCode::C62).observed_at("2020-09-09T16:40:00Z".parse()?)
        )
    );
    expect_that!(attribute_members(entity, &sub_category), some(eq(2)));
    expect_that!(entity, has_attribute(&sub_category, any_attribute().value("tourism")));
    // Full IRIs under a user @context that defines the attributes: an absolute IRI expands to itself (5.5.7), so the
    // request selects the entity carrying that attribute exactly as it does without the Link header
    let context = Context::Single(ContextEntry::Inline(
        format!(
            r#"{{"Building_{namespace}": "https://test.andromeda/{namespace}/Building", "airQualityLevel": "https://test.andromeda/{namespace}/airQualityLevel", "subCategory": "https://test.andromeda/{namespace}/subCategory"}}"#
        )
        .parse::<InlineContext>()?,
    ));
    let hosted = HostedContext::add(&broker, &context).await?;
    let linked = broker.with_context(Context::Single(hosted.entry()?))?;
    let entities_2 = [
        Entity::builder()
            .id(building_3_id.clone())
            .entity_type(building_type.clone())
            .property("subCategory", "tourism")
            .property("airQualityLevel", 4)
            .build()?,
        Entity::builder().id(building_4_id).entity_type(building_type.clone()).build()?,
    ];
    linked.entities().batch_create(&entities_2).send().await?.done()?;
    let page_2 = linked
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .attrs(vec![
            format!("https://test.andromeda/{namespace}/airQualityLevel").parse::<AttributeName>()?,
            format!("https://test.andromeda/{namespace}/subCategory").parse::<AttributeName>()?,
        ])
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(1)));
    let entity_2 = page_2.items().first().or_fail()?;
    expect_that!(entity_2, all!(has_id(&building_3_id), has_type(&building_type)));
    expect_that!(member_count(entity_2), eq(4));
    expect_that!(entity_2, has_attribute("airQualityLevel", any_attribute().value(4)));
    expect_that!(entity_2, has_attribute("subCategory", any_attribute().value("tourism")));
    Ok(())
}
