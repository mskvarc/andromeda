//! A typed date time member of an array matches its instant in every bare spelling.

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

/// Pins: Query Entities with a bare q equal to a `DateTime` selects the Entity whose array Property value or
/// `valueList` holds that instant typed `DateTime`, whatever fraction spelling either side was written in, no other
/// instant and no quoted operand, while the members are served typed in their one spelling
/// Clause: 4.9, 4.6.3, C.6
#[gtest]
#[tokio::test]
async fn n_qry_qlg_9057_a_typed_date_time_member_of_an_array_matches_its_instant_in_every_bare_spelling() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let gate_1_id = namespace.urn("Gate", 1)?;
    let gate_2_id = namespace.urn("Gate", 2)?;
    let gate_type = namespace.entity_type("Gate")?;
    let gate_1 = Entity::builder()
        .id(gate_1_id.clone())
        .entity_type(gate_type.clone())
        .property(
            "openings",
            Property::builder(Value::from(vec![
                Value::Typed(TypedValue::try_new(CoreDatatype::DateTime, "2024-01-02T00:00:00.000Z")?),
                Value::from("north"),
            ])),
        )
        .build()?;
    // The comma fraction is a spelling the model refuses, so the closings go out as written.
    let closings: RawJson = r#"{"type": "ListProperty", "valueList": [{"@type": "DateTime", "@value": "2024-01-02T18:00:00.000Z"}, {"@type": "DateTime", "@value": "2024-01-03T18:00:00,5Z"}]}"#.parse()?;
    let document = UncheckedDocument::from_entity(&gate_1, Lossless::Normalized).set_member("closings", &closings);
    let request = broker.unchecked(&CreateEntity::builder().entity(&gate_1).build())?.document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let gate_2 = Entity::builder()
        .id(gate_2_id.clone())
        .entity_type(gate_type.clone())
        .property(
            "openings",
            Property::builder(Value::from(vec![
                Value::Typed(TypedValue::try_new(CoreDatatype::DateTime, "2024-01-04T00:00:00Z")?),
                Value::from("south"),
            ])),
        )
        .list_property(
            "closings",
            ListProperty::builder(vec![Value::Typed(TypedValue::try_new(CoreDatatype::DateTime, "2024-01-04T18:00:00Z")?)]),
        )
        .build()?;
    broker.entities().create(&gate_2).send().await?.done()?;
    expect_that!(
        selected_ids(&broker, &gate_type, "openings==2024-01-02T00:00:00Z").await?,
        elements_are![eq(&gate_1_id)]
    );
    expect_that!(
        selected_ids(&broker, &gate_type, "openings==2024-01-02T00:00:00.000000Z").await?,
        elements_are![eq(&gate_1_id)]
    );
    expect_that!(selected_ids(&broker, &gate_type, r#"openings=="2024-01-02T00:00:00.0Z""#).await?, is_empty());
    expect_that!(selected_ids(&broker, &gate_type, "openings==2024-01-02T00:00:00.001Z").await?, is_empty());
    expect_that!(
        selected_ids(&broker, &gate_type, "closings==2024-01-02T18:00:00Z").await?,
        elements_are![eq(&gate_1_id)]
    );
    expect_that!(
        selected_ids(&broker, &gate_type, "closings==2024-01-03T18:00:00.500000Z").await?,
        elements_are![eq(&gate_1_id)]
    );
    expect_that!(
        selected_ids(&broker, &gate_type, "closings==2024-01-04T18:00:00.000Z").await?,
        elements_are![eq(&gate_2_id)]
    );
    // The spelling of the served members is the subject, so the answer is read as received.
    let read = request_to(&broker, Method::GET, &format!("entities/{gate_1_id}"))?.append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let response = send_raw(read).await?;
    expect_that!(response.status(), eq(StatusCode::OK));
    expect_that!(response.body(), json_equals("$.openings.value[0].type", "DateTime"));
    expect_that!(response.body(), json_equals("$.openings.value[0]['@value']", "2024-01-02T00:00:00Z"));
    expect_that!(response.body(), json_equals("$.closings.valueList[0]['@value']", "2024-01-02T18:00:00Z"));
    expect_that!(response.body(), json_equals("$.closings.valueList[1]['@value']", "2024-01-03T18:00:00.500Z"));
    Ok(())
}
