//! A quoted operand with another fraction spelling selects nothing.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, q_counts::observed};
use googletest::prelude::*;
use ngsi_ld::model::Entity;

/// Pins: Query Entities with q comparing readingTime 12:00:00Z and readingInstant 2024-01-01T00:00:00Z to quoted
/// operands spelled as stored selects the Entity, and to quoted operands with a short fraction or six fraction
/// digits selects nothing
/// Clause: 4.6.3, 4.9
/// Recorded: clause 4.9 makes quotedStr a String, compared as text (IETF RFC 8259 section 8.3), and a target value of
/// another data type does not match it; whether the stored string is read as a String or as a time value, a quoted
/// operand spelled otherwise matches nothing
#[gtest]
#[tokio::test]
async fn o_qry_qlg_9041_a_quoted_operand_with_another_fraction_spelling_selects_nothing() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let observation_id = namespace.urn("Observation", "admitted")?;
    let observation_type = namespace.entity_type("Observation")?;
    let observation = Entity::builder()
        .id(observation_id.clone())
        .entity_type(observation_type)
        .property("readingTime", "12:00:00Z")
        .property("readingInstant", "2024-01-01T00:00:00Z")
        .property("readingDate", "2024-06-15")
        .property("readingLabel", "2024-01-01T01:00:00+01:00")
        .property("readingNote", "12:00:00 in the morning")
        .build()?;
    broker.entities().create(&observation).send().await?.done()?;
    let table = [
        (r#"readingTime=="12:00:00Z""#, 1),
        (r#"readingTime=="12:00:00.0Z""#, 0),
        (r#"readingTime=="12:00:00.000000Z""#, 0),
        (r#"readingInstant=="2024-01-01T00:00:00Z""#, 1),
        (r#"readingInstant=="2024-01-01T00:00:00.000000Z""#, 0),
    ];
    let counts = observed(&broker, &observation_id, &table).await?;
    expect_that!(counts, eq(&table.to_vec()));
    Ok(())
}
