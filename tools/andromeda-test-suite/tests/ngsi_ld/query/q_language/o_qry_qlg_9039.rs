//! A quoted operand with a comma or a shorter fraction selects nothing.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, q_counts::observed};
use googletest::prelude::*;
use ngsi_ld::model::Entity;

/// Pins: Query Entities with q comparing readingTime 12:00:00.500000Z and readingInstant 2024-01-01T00:00:00.250000Z to
/// quoted operands with a comma or a decimal point and fewer fraction digits selects nothing for all four
/// Clause: 4.6.3, 4.9
/// Recorded: clause 4.9 makes quotedStr a String, compared as text (IETF RFC 8259 section 8.3), and a target value of
/// another data type does not match it; whether the stored string is read as a String or as a time value, a quoted
/// operand spelled otherwise matches nothing
#[gtest]
#[tokio::test]
async fn o_qry_qlg_9039_a_quoted_operand_with_a_comma_or_a_shorter_fraction_selects_nothing() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let observation_id = namespace.urn("Observation", "comma")?;
    let observation_type = namespace.entity_type("Observation")?;
    let observation = Entity::builder()
        .id(observation_id.clone())
        .entity_type(observation_type)
        .property("readingTime", "12:00:00.500000Z")
        .property("readingInstant", "2024-01-01T00:00:00.250000Z")
        .property("readingDate", "2024-06-15")
        .property("readingLabel", "2024-01-01T01:00:00+01:00")
        .property("readingNote", "12:00:00 in the morning")
        .build()?;
    broker.entities().create(&observation).send().await?.done()?;
    let table = [
        (r#"readingTime=="12:00:00,5Z""#, 0),
        (r#"readingTime=="12:00:00.5Z""#, 0),
        (r#"readingInstant=="2024-01-01T00:00:00,25Z""#, 0),
        (r#"readingInstant=="2024-01-01T00:00:00.25Z""#, 0),
    ];
    let counts = observed(&broker, &observation_id, &table).await?;
    expect_that!(counts, eq(&table.to_vec()));
    Ok(())
}
