//! A prefixed name or an absolute IRI in q goes in double quotes.

use andromeda_test_suite::{
    broker,
    hosted_context::HostedContext,
    identity_matcher::ids_in_order,
    namespace::Namespace,
    problem::{answered, problem},
    quoted_name_query::{context_link, query_by_text},
    raw_call::target,
    unchecked_call::exchange,
    unchecked_page::entities,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use iri_rs::IriRefBuf;
use ngsi_ld::{
    http::ContextDelivery,
    model::{AttributeName, Context, ContextEntry, Entity, ErrorType, InlineContext},
};
use std::slice;

/// Pins: with an @context holding the prefix ex, q="ex:speed">50, q="https://example.org/<namespace>/speed">50 and,
/// under an @context holding the term speed for that IRI, q=speed>50 each select the Car whose speed is 80, and
/// q="urn:x:<namespace>:weight">1000 selects the Car whose weight is 2000; q=ex:speed>50, q=urn:x:<namespace>:weight>1000
/// and q=https://example.org/<namespace>/speed>50 are each 400 `BadRequestData`
/// Clause: 4.9, 4.6.2, 5.5.7, 5.7.2.4, 6.4.3.2
/// Recorded: clause 4.9 spells `AttrName` as a letter followed by letters, digits and underscores, and has every name
/// expanded as clause 5.5.7 says, which reads a prefixed name and an absolute IRI too; Andromeda keeps all three kinds
/// of name, reads an unquoted name as exactly the grammar's `AttrName` and any other name as the grammar's
/// `quotedStr`, whose value is the name, and refuses an unquoted name holding a colon
#[gtest]
#[tokio::test]
async fn a_qry_qlg_9068_a_prefixed_name_or_an_absolute_iri_in_q_goes_in_double_quotes() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let car_type = namespace.entity_type("Car")?;
    let fast_id = namespace.urn("Car", "fast")?;
    let slow_id = namespace.urn("Car", "slow")?;
    let prefixed = HostedContext::add(
        &broker,
        &Context::Single(ContextEntry::Inline(
            format!(r#"{{"ex":"https://example.org/{namespace}/"}}"#).parse::<InlineContext>()?,
        )),
    )
    .await?;
    let termed = HostedContext::add(
        &broker,
        &Context::Single(ContextEntry::Inline(
            format!(r#"{{"speed":"https://example.org/{namespace}/speed"}}"#).parse::<InlineContext>()?,
        )),
    )
    .await?;
    let writer = broker
        .with_context(Context::Multiple(vec![
            ContextEntry::Inline(format!(r#"{{"ex":"https://example.org/{namespace}/"}}"#).parse::<InlineContext>()?),
            ContextEntry::Reference(IriRefBuf::new("https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld".to_owned())?),
        ]))?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let speed = "ex:speed".parse::<AttributeName>()?;
    let weight = format!("urn:x:{namespace}:weight").parse::<AttributeName>()?;
    let cars = [
        Entity::builder()
            .id(fast_id.clone())
            .entity_type(car_type.clone())
            .property(speed.clone(), 80)
            .property(weight.clone(), 900)
            .build()?,
        Entity::builder()
            .id(slow_id.clone())
            .entity_type(car_type.clone())
            .property(speed, 20)
            .property(weight, 2000)
            .build()?,
    ];
    let created = writer.with_responses().entities().batch_create(&cars).send().await;
    expect_that!(&created, answered(StatusCode::CREATED));
    let prefixed_link = context_link(&prefixed)?;
    let termed_link = context_link(&termed)?;
    let iri = format!(r#""https://example.org/{namespace}/speed">50"#);
    // The q model writes a name only as an unquoted AttrName, so each q is sent as written.
    for (q, link) in [
        (r#""ex:speed">50"#, Some(&prefixed_link)),
        (iri.as_str(), None),
        ("speed>50", Some(&termed_link)),
    ] {
        let answer = query_by_text(&broker, &car_type, q, &[], link).await?;
        expect_that!(answer.status(), eq(StatusCode::OK), "{q}");
        expect_that!(entities(&answer)?.as_slice(), ids_in_order(slice::from_ref(&fast_id)), "{q}");
    }
    let heavy = format!(r#""urn:x:{namespace}:weight">1000"#);
    let answer = query_by_text(&broker, &car_type, &heavy, &[], None).await?;
    expect_that!(answer.status(), eq(StatusCode::OK));
    expect_that!(entities(&answer)?.as_slice(), ids_in_order(slice::from_ref(&slow_id)));
    let car_type_text = car_type.to_string();
    for q in [
        "ex:speed>50".to_owned(),
        format!("urn:x:{namespace}:weight>1000"),
        format!("https://example.org/{namespace}/speed>50"),
    ] {
        let refused = broker
            .send_unchecked(exchange(
                &broker,
                Method::GET,
                &target("entities", &[("type", car_type_text.as_str()), ("q", q.as_str())]),
                &[("Accept", "application/json")],
                None,
            )?)
            .await;
        expect_that!(&refused, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData), "{q}");
    }
    Ok(())
}
