//! A typed time member of an array matches its time of day in every bare spelling.

use andromeda_test_suite::{
    body_json::json_equals,
    broker,
    namespace::Namespace,
    outcome::Done,
    problem::answered,
    q_selection::selected_ids,
    raw_answer::send_raw,
    wire::request_to,
};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode, header::ACCEPT};
use ngsi_ld::{
    http::{UncheckedDocument, operation::CreateEntity},
    model::{CoreDatatype, Entity, ListProperty, Lossless, Property, RawJson, TypedValue, Value},
};

/// Pins: Query Entities with a bare q equal to a Time selects the Entity whose array Property value or `valueList`
/// holds that time of day typed Time, whatever fraction spelling either side was written in, and no other time of
/// day, while the members are served typed in their one spelling
/// Clause: 4.9, 4.6.3, C.6
#[gtest]
#[tokio::test]
async fn n_qry_qlg_9058_a_typed_time_member_of_an_array_matches_its_time_of_day_in_every_bare_spelling() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let shift_1_id = namespace.urn("Shift", 1)?;
    let shift_2_id = namespace.urn("Shift", 2)?;
    let shift_type = namespace.entity_type("Shift")?;
    let shift_1 = Entity::builder()
        .id(shift_1_id.clone())
        .entity_type(shift_type.clone())
        .property(
            "starts",
            Property::builder(Value::from(vec![
                Value::Typed(TypedValue::try_new(CoreDatatype::Time, "12:00:00.000Z")?),
                Value::Typed(TypedValue::try_new(CoreDatatype::Time, "13:00:00Z")?),
            ])),
        )
        .list_property(
            "breaks",
            ListProperty::builder(vec![
                Value::Typed(TypedValue::try_new(CoreDatatype::Time, "10:15:00.000Z")?),
                Value::Typed(TypedValue::try_new(CoreDatatype::Time, "15:15:00Z")?),
            ]),
        )
        .build()?;
    // The comma fraction is a spelling the model refuses, so `starts` goes out as written.
    let starts: RawJson =
        r#"{"type": "Property", "value": [{"@type": "Time", "@value": "12:00:00.000Z"}, {"@type": "Time", "@value": "13:00:00,5Z"}]}"#.parse()?;
    let document = UncheckedDocument::from_entity(&shift_1, Lossless::Normalized).set_member("starts", &starts);
    let request = broker.unchecked(&CreateEntity::builder().entity(&shift_1).build())?.document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let shift_2 = Entity::builder()
        .id(shift_2_id.clone())
        .entity_type(shift_type.clone())
        .property(
            "starts",
            Property::builder(Value::from(vec![
                Value::Typed(TypedValue::try_new(CoreDatatype::Time, "14:00:00Z")?),
                Value::Typed(TypedValue::try_new(CoreDatatype::Time, "15:00:00Z")?),
            ])),
        )
        .list_property(
            "breaks",
            ListProperty::builder(vec![Value::Typed(TypedValue::try_new(CoreDatatype::Time, "16:15:00.000000Z")?)]),
        )
        .build()?;
    broker.entities().create(&shift_2).send().await?.done()?;
    expect_that!(selected_ids(&broker, &shift_type, "starts==12:00:00Z").await?, elements_are![eq(&shift_1_id)]);
    expect_that!(
        selected_ids(&broker, &shift_type, "starts==12:00:00.000Z").await?,
        elements_are![eq(&shift_1_id)]
    );
    expect_that!(selected_ids(&broker, &shift_type, "starts==13:00:00.5Z").await?, elements_are![eq(&shift_1_id)]);
    expect_that!(selected_ids(&broker, &shift_type, "starts==12:00:00.001Z").await?, is_empty());
    expect_that!(selected_ids(&broker, &shift_type, "breaks==10:15:00Z").await?, elements_are![eq(&shift_1_id)]);
    expect_that!(selected_ids(&broker, &shift_type, "breaks==16:15:00.0Z").await?, elements_are![eq(&shift_2_id)]);
    // The spelling of the served members is the subject, so the answer is read as received.
    let read = request_to(&broker, Method::GET, &format!("entities/{shift_1_id}"))?.append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let response = send_raw(read).await?;
    expect_that!(response.status(), eq(StatusCode::OK));
    expect_that!(response.body(), json_equals("$.starts.value[0].type", "Time"));
    expect_that!(response.body(), json_equals("$.starts.value[0]['@value']", "12:00:00Z"));
    expect_that!(response.body(), json_equals("$.starts.value[1]['@value']", "13:00:00.500Z"));
    expect_that!(response.body(), json_equals("$.breaks.valueList[0]['@value']", "10:15:00Z"));
    Ok(())
}
