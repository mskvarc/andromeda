//! Query Entities by POST reads a prefixed name or a dot-free IRI in q alike unquoted or quoted.

use andromeda_test_suite::{
    broker,
    hosted_context::HostedContext,
    identity_matcher::ids_in_order,
    namespace::Namespace,
    problem::answered,
    written_parameters::send_written_body,
};
use googletest::prelude::*;
use http::StatusCode;
use iri_rs::IriRefBuf;
use ngsi_ld::{
    http::{
        ContextDelivery,
        operation::{QueryEntities, QueryMethod},
    },
    model::{AttributeName, Context, ContextEntry, Entity, InlineContext, Lossless},
    query::builders::SelectorTypeBuilder,
};
use std::slice;

/// Pins: Query Entities through POST `/entityOperations/query` under an @context holding the prefix ex, with the q
/// ex:speed>50 and "ex:speed">50 in its body, selects the Car whose ex:speed is 80, and with the q
/// urn:x:<namespace>:weight>1000 and "urn:x:<namespace>:weight">1000 selects the Car whose weight is 2000
/// Clause: 4.9, 4.6.2, 5.2.23, 5.7.2
/// Recorded: the `q` of a clause 5.2.23 Query is the clause 4.9 text the query string carries; Andromeda reads an
/// unquoted name through `-`, `:`, `/` and `#` wherever an unquoted colon has no meaning of its own, in either binding
#[gtest]
#[tokio::test]
async fn a_qry_qlg_9075_a_posted_q_reads_a_prefixed_name_or_a_dot_free_iri_alike_unquoted_or_quoted() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let car_type = namespace.entity_type("Car")?;
    let fast_id = namespace.urn("Car", "fast")?;
    let slow_id = namespace.urn("Car", "slow")?;
    let prefix = format!(r#"{{"ex":"https://example.org/{namespace}/"}}"#);
    let prefixed = HostedContext::add(&broker, &Context::Single(ContextEntry::Inline(prefix.parse::<InlineContext>()?))).await?;
    let writer = broker
        .with_context(Context::Multiple(vec![
            ContextEntry::Inline(prefix.parse::<InlineContext>()?),
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
    let posted = QueryEntities::builder()
        .entity_type(&selection)
        .method(QueryMethod::Post)
        .format(Lossless::Normalized)
        .build();
    let reader = broker.with_context(Context::Single(prefixed.entry()?))?;
    let heavy = format!("urn:x:{namespace}:weight>1000");
    let quoted_heavy = format!(r#""urn:x:{namespace}:weight">1000"#);
    // The q model writes a name only as an unquoted AttrName, so the q of the body is set as written.
    for (q, selected) in [
        ("ex:speed>50", &fast_id),
        (r#""ex:speed">50"#, &fast_id),
        (heavy.as_str(), &slow_id),
        (quoted_heavy.as_str(), &slow_id),
    ] {
        let page = send_written_body(&reader, &posted, &[("q", q)]).await?;
        expect_that!(page.items(), ids_in_order(slice::from_ref(selected)), "{q}");
    }
    Ok(())
}
