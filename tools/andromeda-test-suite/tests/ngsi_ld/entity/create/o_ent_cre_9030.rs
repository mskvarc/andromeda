//! An empty key or type inside a value is bad request data.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem, value::object};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::model::{Entity, ErrorType, Value};

/// Pins: Create Entity with a value holding an empty key answers 400 `BadRequestData` naming /p/value/, and with a
/// value holding an empty type 400 `BadRequestData` naming /p/value/type
/// Clause: 4.6.4, 5.5.4
/// Recorded: 4.6.4 asks for a value back as written and JSON-LD expansion drops a key of keyword form and maps an
/// empty key or an empty type to the default vocabulary; Andromeda refuses such a key with `BadRequestData` naming
/// where it stands rather than store an altered value
#[gtest]
#[tokio::test]
async fn o_ent_cre_9030_an_empty_key_or_type_inside_a_value_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let shop_empty_key_id = namespace.urn("Shop", "empty-key")?;
    let shop_type = namespace.entity_type("Shop")?;
    let shop_empty_type_id = namespace.urn("Shop", "empty-type")?;
    let shop_empty_key = Entity::builder()
        .id(shop_empty_key_id)
        .entity_type(shop_type.clone())
        .property("p", object([("", Value::from("x"))]))
        .build()?;
    let answer = broker.with_responses().entities().create(&shop_empty_key).send().await;
    expect_that!(
        &answer,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail_containing("/p/value/")
    );
    let shop_empty_type = Entity::builder()
        .id(shop_empty_type_id)
        .entity_type(shop_type.clone())
        .property("p", object([("type", Value::from("")), ("street", Value::from("x"))]))
        .build()?;
    let answer_2 = broker.with_responses().entities().create(&shop_empty_type).send().await;
    expect_that!(
        &answer_2,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail_containing("/p/value/type")
    );
    Ok(())
}
