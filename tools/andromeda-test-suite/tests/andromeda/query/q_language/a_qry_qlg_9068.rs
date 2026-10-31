//! A path name may be a prefixed name or an absolute iri.

use andromeda_test_suite::{
    broker,
    identity_matcher::ids_in_order,
    namespace::Namespace,
    problem::{answered, problem},
    raw_call::{call, target},
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

/// Pins: a q path name may be a prefixed name the @context defines, an absolute IRI written bare when it holds no dot,
/// or a name in double quotes, which an IRI holding a dot needs; each selects what the short-hand term selects, and a
/// dotted IRI left bare is `BadRequestData`
/// Clause: 4.9, 4.6.2, 5.7.2.4, 6.4.3.2, 6.23.3.1
/// Recorded: clause 4.9 spells `AttrName` as a letter followed by letters, digits and underscores and calls a path
/// "always a composition of short hand names"; Andromeda reads a path name with the clause 4.6.2 name grammar, prefixed
/// form and absolute IRI included, and admits a quoted name so an IRI holding a dot reads as one name
#[gtest]
#[tokio::test]
async fn a_qry_qlg_9068_a_path_name_may_be_a_prefixed_name_or_an_absolute_iri() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let car_type = namespace.entity_type("Car")?;
    let fast_id = namespace.urn("Car", "fast")?;
    let slow_id = namespace.urn("Car", "slow")?;
    let context = format!(r#"[{{"ex":"https://example.org/{namespace}/"}},"https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld"]"#);
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
            .id(slow_id)
            .entity_type(car_type.clone())
            .property(speed, 20)
            .property(weight, 2000)
            .build()?,
    ];
    let created = writer.with_responses().entities().batch_create(&cars).send().await;
    expect_that!(&created, answered(StatusCode::CREATED));
    // The q model refuses a prefixed name, bare or in double quotes, so the Query document is written as text.
    for q in ["ex:speed>50", r#""ex:speed">50"#] {
        let query = format!(
            r#"{{"type":"Query","entities":[{{"type":"{car_type}"}}],"q":{},"@context":{context}}}"#,
            serde_json::Value::from(q)
        );
        let answer = call(
            &broker,
            Method::POST,
            "entityOperations/query",
            &[("Content-Type", "application/ld+json"), ("Accept", "application/json")],
            Some(&query),
        )
        .await?;
        expect_that!(answer.status(), eq(StatusCode::OK));
        expect_that!(entities(&answer)?.as_slice(), ids_in_order(slice::from_ref(&fast_id)));
    }
    let car_type_text = car_type.to_string();
    // The q model refuses a path name that is an absolute IRI, so the queries are sent as written.
    let dotted = format!(r#""https://example.org/{namespace}/speed">50"#);
    let dotted_bare = format!("https://example.org/{namespace}/speed>50");
    let undotted = format!("urn:x:{namespace}:weight<1000");
    let undotted_quoted = format!(r#""urn:x:{namespace}:weight"<1000"#);
    for q in [&dotted, &undotted, &undotted_quoted] {
        let answer = call(
            &broker,
            Method::GET,
            &target("entities", &[("type", car_type_text.as_str()), ("q", q.as_str())]),
            &[("Accept", "application/json")],
            None,
        )
        .await?;
        expect_that!(answer.status(), eq(StatusCode::OK));
        expect_that!(entities(&answer)?.as_slice(), ids_in_order(slice::from_ref(&fast_id)));
    }
    // The q model refuses a path name that is an absolute IRI, so the query is sent as written.
    let refused = broker
        .send_unchecked(exchange(
            &broker,
            Method::GET,
            &target("entities", &[("type", car_type_text.as_str()), ("q", dotted_bare.as_str())]),
            &[("Accept", "application/json")],
            None,
        )?)
        .await;
    expect_that!(&refused, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    Ok(())
}
