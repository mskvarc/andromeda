//! A quoted IRI holding a comma or a semicolon is one name of an orderBy term.

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
    http::{QueryParameter, operation::QueryEntities},
    model::{Context, ContextEntry, Entity, InlineContext, Lossless},
    query::builders::SelectorTypeBuilder,
};

/// Pins: over Cars whose attribute rank, under an @context mapping it to urn:x:<namespace>:rank;(a,b), is 10, 20 and
/// 30, Query Entities with no @context and orderBy="urn:x:<namespace>:rank;(a,b)" orders them by rank ascending and
/// with orderBy="urn:x:<namespace>:rank;(a,b)";desc descending, the `;` and the `,` the IRI holds being part of the
/// name
/// Clause: 4.9, 4.23.3, 5.5.7, 6.4.3.2
/// Recorded: clause 4.23.3 reads an `orderBy` path with the clause 4.9 `AttrName`; Andromeda spells an absolute IRI
/// there as it does in q, as a `quotedStr`, and splits the list and the direction only at a mark outside it
#[gtest]
#[tokio::test]
async fn a_qry_ord_9100_a_quoted_iri_holding_a_comma_or_a_semicolon_is_one_name_of_an_order_by_term() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let iri = format!("urn:x:{namespace}:rank;(a,b)");
    let termed = HostedContext::add(
        &broker,
        &Context::Single(ContextEntry::Inline(format!(r#"{{"rank":"{iri}"}}"#).parse::<InlineContext>()?)),
    )
    .await?;
    let writer = broker.with_context(Context::Single(termed.entry()?))?;
    let car_type = namespace.entity_type("Car")?;
    let ascending = [namespace.urn("Car", "low")?, namespace.urn("Car", "middle")?, namespace.urn("Car", "high")?];
    for (id, rank) in ascending.iter().zip([10, 20, 30]) {
        let car = Entity::builder().id(id.clone()).entity_type(car_type.clone()).property("rank", rank).build()?;
        writer.entities().create(&car).send().await?.done()?;
    }
    let mut descending = ascending.clone();
    descending.reverse();
    let selection = SelectorTypeBuilder::from(&car_type).build()?;
    let query = QueryEntities::builder().entity_type(&selection).format(Lossless::Normalized).build();
    let quoted = format!(r#""{iri}""#);
    let quoted_descending = format!(r#""{iri}";desc"#);
    // The ordering model writes a name only as an unquoted AttrName, so an orderBy naming an IRI is set as written.
    for (order_by, expected) in [(quoted.as_str(), &ascending), (quoted_descending.as_str(), &descending)] {
        let page = send_written(&broker, &query, &[(QueryParameter::OrderBy, order_by)]).await?;
        expect_that!(page.items(), ids_in_order(expected), "{order_by}");
    }
    Ok(())
}
