//! A required member missing from a body is named with where it is missing.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{
        UncheckedDocument,
        operation::{CreateEntity, QueryEntities, QueryMethod},
    },
    model::{Entity, ErrorType, Lossless, RawJson},
    query::builders::SelectorTypeBuilder,
};

/// Pins: Create Entity without `id`, without `type`, or with a Property carrying no `value` answers 400
/// `BadRequestData` whose detail names the missing member and the place it is missing at, as a JSON Pointer, and
/// Query Entities by POST with a Query that carries no `type` answers 400 `BadRequestData` naming `type`; no detail
/// says "node", "field" or a line and column of the body
/// Clause: 5.2.4, 5.2.5, 5.2.23, 5.5.4, 5.6.1.4, 5.7.2.4
/// Recorded: clause 5.5.4 fixes the error type but not the wording; Andromeda names the member and where it is
/// missing in the members of the request
#[gtest]
#[tokio::test]
async fn o_cmn_val_9042_a_required_member_missing_from_a_body_is_named_with_where_it_is_missing() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle = Entity::builder()
        .id(namespace.urn("Vehicle", 1)?)
        .entity_type(vehicle_type.clone())
        .property("speed", 55)
        .build()?;
    let written = UncheckedDocument::from_entity(&vehicle, Lossless::Normalized);
    // The model requires `value` of a Property, so the Property is written as text.
    let speed: RawJson = r#"{"type": "Property"}"#.parse()?;
    for (document, detail) in [
        (
            written.clone().remove_member("id"),
            "the request body does not decode: the value at / is refused: the member 'id' is missing",
        ),
        (
            written.clone().remove_member("type"),
            "the request body does not decode: the value at / is refused: the member 'type' is missing",
        ),
        (
            written.clone().set_member("speed", &speed),
            "the request body does not decode: the value at /speed is refused: the member 'value' is missing",
        ),
    ] {
        let request = broker.unchecked(&CreateEntity::builder().entity(&vehicle).build())?.document(&document);
        let answer = broker.send_unchecked(request).await;
        expect_that!(
            &answer,
            problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail(detail),
            "{detail}"
        );
    }
    let selection = SelectorTypeBuilder::from(&vehicle_type).build()?;
    let query: RawJson = format!(r#"{{"type": "Query", "entities": [{{"type": "{vehicle_type}"}}]}}"#).parse()?;
    // A Query without `type` is no Query the model writes, so it is written as text.
    let document = UncheckedDocument::try_from(query)?.remove_member("type");
    let request = broker
        .unchecked(
            &QueryEntities::builder()
                .entity_type(&selection)
                .method(QueryMethod::Post)
                .format(Lossless::Normalized)
                .build(),
        )?
        .document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(
        &answer,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail("the request body does not decode: the member 'type' is missing")
    );
    Ok(())
}
