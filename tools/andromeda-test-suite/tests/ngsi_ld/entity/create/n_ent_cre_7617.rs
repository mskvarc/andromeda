//! A value outside its recognised value type is bad request data.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    outcome::Done,
    problem::problem,
    value::object,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, ErrorType, LanguageMap, LanguageProperty, ListProperty, NgsiLdMediaType, Property, Value, VocabTerm},
};

/// Pins: Create Entity answers 400 `BadRequestData` for abc and 1.5 beside `valueType` xsd:integer, for a
/// `ListProperty` item 1.5 beside it and for a `LanguageProperty` `valueType` other than `langString`, and creates
/// the annex C Integer `ListProperty` and the C.10 `PostalAddress` object unchanged
/// Clause: 4.5.2, 4.5.21, 5.2.32, 5.6.1, C.10
/// Recorded: clause 4.5.2 coerces `valueType` into a datatype URI aligned with the RDF datatype; Andromeda holds a
/// value to the XSD datatypes RDF 1.1 section 5.1 lists and to the NGSI-LD time values, and checks no other
/// datatype IRI, as RDF 1.1 section 5.3 makes only a recognised datatype ill-typed
#[gtest]
#[tokio::test]
async fn n_ent_cre_7617_a_value_outside_its_recognised_value_type_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let counter_text_id = namespace.urn("Counter", "text")?;
    let counter_type = namespace.entity_type("Counter")?;
    let counter_fraction_id = namespace.urn("Counter", "fraction")?;
    let counter_list_id = namespace.urn("Counter", "list")?;
    let counter_language_id = namespace.urn("Counter", "language")?;
    let counter_annex_c_id = namespace.urn("Counter", "annex-c")?;
    let counter_text = Entity::builder()
        .id(counter_text_id)
        .entity_type(counter_type.clone())
        .property(
            "count",
            Property::builder("abc").value_type("http://www.w3.org/2001/XMLSchema#integer".parse()?),
        )
        .build()?;
    let answer = broker.with_responses().entities().create(&counter_text).send().await;
    expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let counter_fraction = Entity::builder()
        .id(counter_fraction_id)
        .entity_type(counter_type.clone())
        .property(
            "count",
            Property::builder(Value::Number("1.5".parse()?)).value_type("http://www.w3.org/2001/XMLSchema#integer".parse()?),
        )
        .build()?;
    let answer_2 = broker.with_responses().entities().create(&counter_fraction).send().await;
    expect_that!(&answer_2, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let counter_list = Entity::builder()
        .id(counter_list_id)
        .entity_type(counter_type.clone())
        .list_property(
            "counts",
            ListProperty::builder(vec![Value::from(300), Value::Number("1.5".parse()?)]).value_type("http://www.w3.org/2001/XMLSchema#integer".parse()?),
        )
        .build()?;
    let answer_3 = broker.with_responses().entities().create(&counter_list).send().await;
    expect_that!(&answer_3, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let counter_language = Entity::builder()
        .id(counter_language_id)
        .entity_type(counter_type.clone())
        .language_property(
            "label",
            LanguageProperty::builder(LanguageMap::try_from_pairs([("en", "Grand Place")])?).value_type("http://www.w3.org/2001/XMLSchema#string".parse()?),
        )
        .build()?;
    let answer_4 = broker.with_responses().entities().create(&counter_language).send().await;
    expect_that!(&answer_4, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&counter_type)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(0)));
    let counter_annex_c = Entity::builder()
        .id(counter_annex_c_id.clone())
        .entity_type(counter_type.clone())
        .list_property(
            "counts",
            ListProperty::builder(vec![Value::from(300), Value::from(300), Value::from(120), Value::from(6)]).value_type("Integer".parse()?),
        )
        .property(
            "address",
            Property::builder(object([("street", Value::from("Pariser Platz")), ("city", Value::from("Berlin"))])).value_type("PostalAddress".parse()?),
        )
        .build()?;
    broker.entities().create(&counter_annex_c).send().await?.done()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&counter_annex_c_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(
        entity,
        has_attribute(
            "counts",
            any_attribute().list_length(4).list_value(3, 6).value_type("Integer".parse::<VocabTerm>()?)
        )
    );
    expect_that!(
        entity,
        has_attribute(
            "address",
            any_attribute()
                .value_member(["city"], "Berlin")
                .value_type("PostalAddress".parse::<VocabTerm>()?)
        )
    );
    Ok(())
}
