//! Q compares every value shape by the value it holds.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, problem::answered, value::object};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::CreateEntity},
    model::{DateTime, Entity, LanguageMap, LanguageProperty, ListProperty, Lossless, NgsiLdMediaType, Number, Property, RawJson, Value},
    query::q,
};

/// Pins: Over one Building holding a number, a String, a Boolean, a `DateTime`, a URL String, a structured value, a
/// `ListProperty`, a `LanguageProperty`, a Relationship, a `ListRelationship` and a sub-Property, each of 28 q
/// expressions (equality, inequality, ordering, a range, a pattern and its negation, existence, a structured member
/// and a sub-attribute) selects the Building exactly when the value it holds satisfies the expression
/// Clause: 4.9, 5.7.2
#[gtest]
#[tokio::test]
async fn n_qry_qlg_9053_q_compares_every_value_shape_by_the_value_it_holds() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_typed_id = namespace.urn("Building", "typed")?;
    let building_type = namespace.entity_type("Building")?;
    let related_id = namespace.urn("Related", 1)?;
    let building_typed = Entity::builder()
        .id(building_typed_id.clone())
        .entity_type(building_type.clone())
        .property("reading", Value::Number("21.5".parse()?))
        .property("category", "alpha")
        .property("active", true)
        .property("seen", Value::from("2024-05-01T00:00:00Z".parse::<DateTime>()?))
        .property("manual", "https://example.test/manual")
        .property("address", object([("city", Value::from("Berlin"))]))
        .list_property("tags", ListProperty::builder(vec![Value::from(1), Value::from(2), Value::from(3)]))
        .language_property(
            "name",
            LanguageProperty::builder(LanguageMap::try_from_pairs([("en", "house"), ("de", "Haus")])?),
        )
        .relationship("link", related_id)
        .property("precise", Property::builder(7).property("accuracy", Value::Number("0.5".parse()?)))
        .build()?;
    // The model refuses it (the model reads it as written otherwise [at $.members.objectList[0] (written "urn:ngsi-ld:Related:zqns-1")]: {"id":"urn:ngsi-ld:Building:zqns-typed","type":"Building_zqns","reading":{"type":"Property","value":21.5},"category":{"type":"Property","value":"alpha"},"active":{"type":"Property","value":true},"seen":{"type":"Property","value":{"@type":"DateTime","@value":"2024-05-01T00:00:00Z"}},"manual":{"type":"Property","value":"https://example.test/manual"},"address":{"type":"Property","value":{"city":"Berlin"}},"tags":{"type":"ListProperty","valueList":[1,2,3]},"name":{"type":"LanguageProperty","languageMap":{"en":"house","de":"Haus"}},"link":{"type":"Relationship","object":"urn:ngsi-ld:Related:zqns-1"},"members":{"type":"ListRelationship","objectList":[{"object":"urn:ngsi-ld:Related:zqns-1"}]},"precise":{"type":"Property","value":7,"accuracy":{"type":"Property","value":0.5}}}), so `members` is written as text.
    let members: RawJson = format!(r#"{{"type": "ListRelationship", "objectList": ["urn:ngsi-ld:Related:{namespace}-1"]}}"#).parse()?;
    let document = UncheckedDocument::from_entity(&building_typed, Lossless::Normalized).set_member("members", &members);
    let request = broker.unchecked(&CreateEntity::builder().entity(&building_typed).build())?.document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .ids(vec![building_typed_id.clone()])
        .q(q::attr("reading").eq("21.5".parse::<Number>()?))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), contains(has_id(&building_typed_id)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .ids(vec![building_typed_id.clone()])
        .q(q::attr("reading").ne("21.5".parse::<Number>()?))
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(0)));
    let page_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .ids(vec![building_typed_id.clone()])
        .q(q::attr("reading").gt(21))
        .send()
        .await?;
    expect_that!(page_3.items(), len(eq(1)));
    expect_that!(page_3.items(), contains(has_id(&building_typed_id)));
    let page_4 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .ids(vec![building_typed_id.clone()])
        .q(q::attr("reading").lt(21))
        .send()
        .await?;
    expect_that!(page_4.items(), len(eq(0)));
    let page_5 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .ids(vec![building_typed_id.clone()])
        .q(q::attr("reading").within(21, 22))
        .send()
        .await?;
    expect_that!(page_5.items(), len(eq(1)));
    expect_that!(page_5.items(), contains(has_id(&building_typed_id)));
    let page_6 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .ids(vec![building_typed_id.clone()])
        .q(q::attr("reading").within(1, 2))
        .send()
        .await?;
    expect_that!(page_6.items(), len(eq(0)));
    let page_7 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .ids(vec![building_typed_id.clone()])
        .q(q::attr("category").eq("alpha"))
        .send()
        .await?;
    expect_that!(page_7.items(), len(eq(1)));
    expect_that!(page_7.items(), contains(has_id(&building_typed_id)));
    let page_8 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .ids(vec![building_typed_id.clone()])
        .q(q::attr("category").eq("beta"))
        .send()
        .await?;
    expect_that!(page_8.items(), len(eq(0)));
    let page_9 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .ids(vec![building_typed_id.clone()])
        .q(q::attr("category").matches("\"lph\""))
        .send()
        .await?;
    expect_that!(page_9.items(), len(eq(1)));
    expect_that!(page_9.items(), contains(has_id(&building_typed_id)));
    let page_10 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .ids(vec![building_typed_id.clone()])
        .q(q::attr("category").not_matches("\"lph\""))
        .send()
        .await?;
    expect_that!(page_10.items(), len(eq(0)));
    let page_11 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .ids(vec![building_typed_id.clone()])
        .q(q::attr("active").eq(true))
        .send()
        .await?;
    expect_that!(page_11.items(), len(eq(1)));
    expect_that!(page_11.items(), contains(has_id(&building_typed_id)));
    let page_12 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .ids(vec![building_typed_id.clone()])
        .q(q::attr("active").eq(false))
        .send()
        .await?;
    expect_that!(page_12.items(), len(eq(0)));
    let page_13 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .ids(vec![building_typed_id.clone()])
        .q(q::attr("seen").eq("2024-05-01T00:00:00Z".parse::<DateTime>()?))
        .send()
        .await?;
    expect_that!(page_13.items(), len(eq(1)));
    expect_that!(page_13.items(), contains(has_id(&building_typed_id)));
    let page_14 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .ids(vec![building_typed_id.clone()])
        .q(q::attr("seen").gt("2024-04-30T00:00:00Z".parse::<DateTime>()?))
        .send()
        .await?;
    expect_that!(page_14.items(), len(eq(1)));
    expect_that!(page_14.items(), contains(has_id(&building_typed_id)));
    let page_15 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .ids(vec![building_typed_id.clone()])
        .q(q::attr("seen").lt("2024-04-30T00:00:00Z".parse::<DateTime>()?))
        .send()
        .await?;
    expect_that!(page_15.items(), len(eq(0)));
    let page_16 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .ids(vec![building_typed_id.clone()])
        .q(q::attr("manual").eq("https://example.test/manual"))
        .send()
        .await?;
    expect_that!(page_16.items(), len(eq(1)));
    expect_that!(page_16.items(), contains(has_id(&building_typed_id)));
    let page_17 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .ids(vec![building_typed_id.clone()])
        .q(q::attr("manual").eq("https://example.test/other"))
        .send()
        .await?;
    expect_that!(page_17.items(), len(eq(0)));
    let page_18 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .ids(vec![building_typed_id.clone()])
        .q(q::attr("link").eq(format!("urn:ngsi-ld:Related:{namespace}-1").as_str()))
        .send()
        .await?;
    expect_that!(page_18.items(), len(eq(1)));
    expect_that!(page_18.items(), contains(has_id(&building_typed_id)));
    let page_19 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .ids(vec![building_typed_id.clone()])
        .q(q::attr("link").eq(format!("urn:ngsi-ld:Related:{namespace}-2").as_str()))
        .send()
        .await?;
    expect_that!(page_19.items(), len(eq(0)));
    let page_20 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .ids(vec![building_typed_id.clone()])
        .q(q::attr("reading").exists())
        .send()
        .await?;
    expect_that!(page_20.items(), len(eq(1)));
    expect_that!(page_20.items(), contains(has_id(&building_typed_id)));
    let page_21 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .ids(vec![building_typed_id.clone()])
        .q(q::attr("missing").exists())
        .send()
        .await?;
    expect_that!(page_21.items(), len(eq(0)));
    let page_22 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .ids(vec![building_typed_id.clone()])
        .q(q::attr("tags").exists())
        .send()
        .await?;
    expect_that!(page_22.items(), len(eq(1)));
    expect_that!(page_22.items(), contains(has_id(&building_typed_id)));
    let page_23 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .ids(vec![building_typed_id.clone()])
        .q(q::attr("name").exists())
        .send()
        .await?;
    expect_that!(page_23.items(), len(eq(1)));
    expect_that!(page_23.items(), contains(has_id(&building_typed_id)));
    let page_24 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .ids(vec![building_typed_id.clone()])
        .q(q::attr("members").exists())
        .send()
        .await?;
    expect_that!(page_24.items(), len(eq(1)));
    expect_that!(page_24.items(), contains(has_id(&building_typed_id)));
    let page_25 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .ids(vec![building_typed_id.clone()])
        .q(q::attr("address").member("city").eq("Berlin"))
        .send()
        .await?;
    expect_that!(page_25.items(), len(eq(1)));
    expect_that!(page_25.items(), contains(has_id(&building_typed_id)));
    let page_26 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .ids(vec![building_typed_id.clone()])
        .q(q::attr("address").member("city").eq("Paris"))
        .send()
        .await?;
    expect_that!(page_26.items(), len(eq(0)));
    let page_27 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .ids(vec![building_typed_id.clone()])
        .q(q::attr("precise").sub_attr("accuracy").eq("0.5".parse::<Number>()?))
        .send()
        .await?;
    expect_that!(page_27.items(), len(eq(1)));
    expect_that!(page_27.items(), contains(has_id(&building_typed_id)));
    let page_28 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&building_type)
        .ids(vec![building_typed_id.clone()])
        .q(q::attr("precise").sub_attr("accuracy").eq("0.6".parse::<Number>()?))
        .send()
        .await?;
    expect_that!(page_28.items(), len(eq(0)));
    Ok(())
}
