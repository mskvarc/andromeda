//! A quoted prefixed name orders as the term for the same IRI does.

use andromeda_test_suite::{
    broker,
    hosted_context::HostedContext,
    identity_matcher::ids_in_order,
    namespace::Namespace,
    outcome::Done,
    problem::problem,
    quoted_name_query::{context_link, get_as_written},
    raw_call::target,
    unchecked_call::exchange,
    unchecked_page::entities,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::model::{Context, ContextEntry, Entity, ErrorType, InlineContext};

/// Pins: orderBy="ex:speed" under an @context holding the prefix ex orders the Cars as orderBy=speed does under an
/// @context holding the term speed for the same IRI, and orderBy=ex:speed is 400 `BadRequestData`
/// Clause: 4.6.2, 4.9, 4.23.3, 6.4.3.2
/// Recorded: clause 4.23.3 reads an `orderBy` path with the clause 4.9 `AttrName`; Andromeda spells a prefixed name
/// there as it does in q, as a `quotedStr`
#[gtest]
#[tokio::test]
async fn a_qry_ord_9014_a_quoted_prefixed_name_orders_as_the_term_for_the_same_iri_does() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let context = |members: String| -> Result<Context> { Ok(Context::Single(ContextEntry::Inline(members.parse::<InlineContext>()?))) };
    let prefixed = HostedContext::add(&broker, &context(format!(r#"{{"ex":"https://example.org/{namespace}/"}}"#))?).await?;
    let termed = HostedContext::add(&broker, &context(format!(r#"{{"speed":"https://example.org/{namespace}/speed"}}"#))?).await?;
    let writer = broker.with_context(Context::Single(termed.entry()?))?;
    let car_type = namespace.entity_type("Car")?;
    let ids = [namespace.urn("Car", "slow")?, namespace.urn("Car", "middle")?, namespace.urn("Car", "fast")?];
    for (id, speed) in ids.iter().zip([20, 50, 80]) {
        let car = Entity::builder()
            .id(id.clone())
            .entity_type(car_type.clone())
            .property("speed", speed)
            .build()?;
        writer.entities().create(&car).send().await?.done()?;
    }
    let car_type_text = car_type.to_string();
    // The ordering model writes a name only as an unquoted AttrName, so each orderBy is sent as written.
    for (order_by, hosted) in [(r#""ex:speed""#, &prefixed), ("speed", &termed)] {
        let answer = get_as_written(
            &broker,
            "entities",
            &[("type", car_type_text.as_str()), ("orderBy", order_by)],
            Some(&context_link(hosted)?),
        )
        .await?;
        expect_that!(answer.status(), eq(StatusCode::OK), "{order_by}");
        expect_that!(entities(&answer)?.as_slice(), ids_in_order(&ids), "{order_by}");
    }
    let refused = broker
        .send_unchecked(exchange(
            &broker,
            Method::GET,
            &target("entities", &[("type", car_type_text.as_str()), ("orderBy", "ex:speed")]),
            &[("Accept", "application/json")],
            None,
        )?)
        .await;
    expect_that!(&refused, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    Ok(())
}
