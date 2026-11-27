//! A prefixed name or an absolute IRI holding no dot in q reads alike unquoted or quoted.

use andromeda_test_suite::{
    broker,
    hosted_context::HostedContext,
    identity_matcher::ids_in_order,
    namespace::Namespace,
    problem::{answered, problem},
    written_parameters::{send_written, written_request},
};
use googletest::prelude::*;
use http::StatusCode;
use iri_rs::IriRefBuf;
use ngsi_ld::{
    http::{ContextDelivery, QueryParameter, operation::QueryEntities},
    model::{AttributeName, Context, ContextEntry, Entity, ErrorType, InlineContext, Lossless},
    query::{builders::SelectorTypeBuilder, q::Query},
};
use std::slice;

/// Pins: with an @context holding the prefix ex, q=ex:speed>50, q="ex:speed">50,
/// q="https://example.org/<namespace>/speed">50 and, under an @context holding the term speed for that IRI,
/// q=speed>50 each select the Car whose speed is 80; q=urn:x:<namespace>:weight>1000 and
/// q="urn:x:<namespace>:weight">1000 each select the Car whose weight is 2000; and
/// q=https://example.org/<namespace>/speed>50, whose IRI holds a dot, is 400 `BadRequestData`
/// Clause: 4.9, 4.6.2, 5.5.7, 5.7.2.4, 6.4.3.2
/// Recorded: clause 4.9 spells `AttrName` as a letter followed by letters, digits and underscores, and has every name
/// expanded as clause 5.5.7 says, which reads a prefixed name and an absolute IRI too; Andromeda reads an unquoted name
/// through `-`, `:`, `/` and `#` wherever an unquoted colon has no meaning of its own, and any name as the grammar's
/// `quotedStr`, whose value is the name, so an unquoted name ends at its first dot and the sub-attribute after it is
/// no name
#[gtest]
#[tokio::test]
async fn a_qry_qlg_9068_a_prefixed_name_or_a_dot_free_iri_in_q_reads_alike_unquoted_or_quoted() -> Result<()> {
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
    let selection = SelectorTypeBuilder::from(&car_type).build()?;
    let query = QueryEntities::builder().entity_type(&selection).format(Lossless::Normalized).build();
    let prefixed_reader = broker.with_context(Context::Single(prefixed.entry()?))?;
    let iri = format!(r#""https://example.org/{namespace}/speed">50"#);
    // The q model writes a name only as an unquoted AttrName, so a q naming a prefixed name or an IRI is set as written.
    for (q, reader) in [
        ("ex:speed>50", &prefixed_reader),
        (r#""ex:speed">50"#, &prefixed_reader),
        (iri.as_str(), &broker),
    ] {
        let page = send_written(reader, &query, &[(QueryParameter::Q, q)]).await?;
        expect_that!(page.items(), ids_in_order(slice::from_ref(&fast_id)), "{q}");
    }
    let speed_above_50: Query = "speed>50".parse()?;
    let termed_query = QueryEntities::builder()
        .entity_type(&selection)
        .q(&speed_above_50)
        .format(Lossless::Normalized)
        .build();
    let termed_page = send_written(&broker.with_context(Context::Single(termed.entry()?))?, &termed_query, &[]).await?;
    expect_that!(termed_page.items(), ids_in_order(slice::from_ref(&fast_id)));
    for heavy in [format!("urn:x:{namespace}:weight>1000"), format!(r#""urn:x:{namespace}:weight">1000"#)] {
        let heavy_page = send_written(&broker, &query, &[(QueryParameter::Q, heavy.as_str())]).await?;
        expect_that!(heavy_page.items(), ids_in_order(slice::from_ref(&slow_id)), "{heavy}");
    }
    let dotted = format!("https://example.org/{namespace}/speed>50");
    let request = written_request(&broker, &query, &[(QueryParameter::Q, dotted.as_str())])?;
    expect_that!(
        &broker.send_unchecked(request).await,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData),
        "{dotted}"
    );
    Ok(())
}
