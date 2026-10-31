//! A key of keyword form is bad request data.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    outcome::Done,
    problem::{answered, problem},
    value::object,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{
        UncheckedDocument,
        headers::Accept,
        operation::{AppendAttributes, CreateEntity},
    },
    model::{Entity, EntityFragment, ErrorType, Lossless, NgsiLdMediaType, RawJson, Value},
};

/// Pins: Create Entity with a value key @foo answers 400 `BadRequestData` naming /p/value/@foo, with @foo on the
/// attribute 400 `BadRequestData`, and Append Entity Attributes with @foo as the attribute name 400
/// `BadRequestData` naming /@foo
/// Clause: 4.6.4, 5.5.4
/// Recorded: 4.6.4 asks for a value back as written and JSON-LD expansion drops a key of keyword form and maps an
/// empty key or an empty type to the default vocabulary; Andromeda refuses such a key with `BadRequestData` naming
/// where it stands rather than store an altered value
#[gtest]
#[tokio::test]
async fn o_ent_cre_9029_a_key_of_keyword_form_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let shop_keyword_in_value_id = namespace.urn("Shop", "keyword-in-value")?;
    let shop_type = namespace.entity_type("Shop")?;
    let shop_keyword_on_attribute_id = namespace.urn("Shop", "keyword-on-attribute")?;
    let shop_keyword_append_id = namespace.urn("Shop", "keyword-append")?;
    let shop_keyword_in_value = Entity::builder()
        .id(shop_keyword_in_value_id.clone())
        .entity_type(shop_type.clone())
        .property("p", object([("@foo", Value::from("x")), ("name", Value::from("y"))]))
        .build()?;
    let answer = broker.with_responses().entities().create(&shop_keyword_in_value).send().await;
    expect_that!(
        &answer,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail_containing("/p/value/@foo")
    );
    let shop_keyword_on_attribute = Entity::builder()
        .id(shop_keyword_on_attribute_id.clone())
        .entity_type(shop_type.clone())
        .build()?;
    // The model refuses it (invalid name at p.@foo), so `p` is written as text.
    let p: RawJson = r#"{"type": "Property", "value": 1, "@foo": 2}"#.parse()?;
    let document = UncheckedDocument::from_entity(&shop_keyword_on_attribute, Lossless::Normalized).set_member("p", &p);
    let request = broker
        .unchecked(&CreateEntity::builder().entity(&shop_keyword_on_attribute).build())?
        .document(&document);
    let answer_2 = broker.send_unchecked(request).await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let shop_keyword_append = Entity::builder()
        .id(shop_keyword_append_id.clone())
        .entity_type(shop_type.clone())
        .property("p", 1)
        .build()?;
    broker.entities().create(&shop_keyword_append).send().await?.done()?;
    let fragment = EntityFragment::builder().build()?;
    // The model refuses it (invalid name at @foo), so `@foo` is written as text.
    let foo: RawJson = r#"{"type": "Property", "value": 1}"#.parse()?;
    let document_2 = UncheckedDocument::try_from(fragment.render(Lossless::Normalized))?.set_member("@foo", &foo);
    let request_2 = broker
        .unchecked(&AppendAttributes::builder().id(&shop_keyword_append_id).fragment(&fragment).build())?
        .document(&document_2);
    let answer_3 = broker.send_unchecked(request_2).await;
    expect_that!(
        &answer_3,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail_containing("/@foo")
    );
    let answer_4 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&shop_keyword_in_value_id)
        .send()
        .await;
    expect_that!(&answer_4, answered(StatusCode::NOT_FOUND));
    let answer_5 = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&shop_keyword_on_attribute_id)
        .send()
        .await;
    expect_that!(&answer_5, answered(StatusCode::NOT_FOUND));
    Ok(())
}
