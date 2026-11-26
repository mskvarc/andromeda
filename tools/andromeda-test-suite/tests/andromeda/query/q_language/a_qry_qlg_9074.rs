//! Two q that differ only in where a quoted name ends are answered apart, whichever was asked first.

use andromeda_test_suite::{
    broker,
    hosted_context::HostedContext,
    identity_matcher::ids_in_order,
    namespace::Namespace,
    outcome::Done,
    written_parameters::send_written,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{
        QueryParameter,
        operation::{QueryEntities, Simplified},
    },
    model::{Context, ContextEntry, Entity, InlineContext, Lossless},
    query::builders::SelectorTypeBuilder,
};
use std::slice;

/// Pins: over two Cars holding the attribute `urn:<token>:a;b` with 80 and 20, Query Entities with
/// q="urn:<token>:a";b<50 selects neither, then q="urn:<token>:a;b"<50 selects the Car holding 20; with
/// options=keyValues the same two q, asked in the other order, select the Car holding 20 and then neither
/// Clause: 4.9, 5.7.2.4
/// Recorded: `;` joins two Query Terms (clause 4.9), so "urn:<token>:a";b<50 asks for an attribute `urn:<token>:a` and
/// a `b` below 50, which no Car holds; Andromeda caches compiled queries, and the two q are asked in turn so that a
/// shared cache entry would answer the second with the first's result
#[gtest]
#[tokio::test]
async fn a_qry_qlg_9074_two_q_differing_only_in_where_a_quoted_name_ends_are_answered_apart() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let iri = format!("urn:{}:a;b", namespace.token());
    let termed = HostedContext::add(
        &broker,
        &Context::Single(ContextEntry::Inline(format!(r#"{{"speed":"{iri}"}}"#).parse::<InlineContext>()?)),
    )
    .await?;
    let writer = broker.with_context(Context::Single(termed.entry()?))?;
    let car_type = namespace.entity_type("Car")?;
    let slow_id = namespace.urn("Car", "slow")?;
    for (id, speed) in [(namespace.urn("Car", "fast")?, 80), (slow_id.clone(), 20)] {
        let car = Entity::builder().id(id).entity_type(car_type.clone()).property("speed", speed).build()?;
        writer.entities().create(&car).send().await?.done()?;
    }
    let one_name = format!(r#""{iri}"<50"#);
    let two_terms = format!(r#""urn:{}:a";b<50"#, namespace.token());
    let selection = SelectorTypeBuilder::from(&car_type).build()?;
    let normalized = QueryEntities::builder().entity_type(&selection).format(Lossless::Normalized).build();
    // The q model writes a name only as an unquoted AttrName, so each q naming the IRI is set as written.
    let first = send_written(&broker, &normalized, &[(QueryParameter::Q, two_terms.as_str())]).await?;
    expect_that!(first.items(), is_empty());
    let second = send_written(&broker, &normalized, &[(QueryParameter::Q, one_name.as_str())]).await?;
    expect_that!(second.items(), ids_in_order(slice::from_ref(&slow_id)));
    let key_values = QueryEntities::builder().entity_type(&selection).format(Simplified).build();
    let third = send_written(&broker, &key_values, &[(QueryParameter::Q, one_name.as_str())]).await?;
    expect_that!(third.items(), ids_in_order(slice::from_ref(&slow_id)));
    let fourth = send_written(&broker, &key_values, &[(QueryParameter::Q, two_terms.as_str())]).await?;
    expect_that!(fourth.items(), is_empty());
    Ok(())
}
