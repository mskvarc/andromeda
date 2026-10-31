//! A string only resembling a time value compares as text.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, q_counts::observed};
use googletest::prelude::*;
use ngsi_ld::model::Entity;

/// Pins: Query Entities with q selects the Entity for readingNote equal to its colon-bearing text and unequal to
/// 09:30:00Z, nothing for the offset-bearing readingLabel compared to a `DateTime` by ==, bare or quoted, and the
/// Entity for readingLabel compared by > and < to quoted operands it follows and precedes as text
/// Clause: 4.6.3, 4.9
/// Recorded: clause 4.6.3 has a `DateTime` end in "Z", so readingLabel is a String; clause 4.9 makes quotedStr a
/// String, compared as text (IETF RFC 8259 section 8.3), and a bare `DateTime` operand of another data type matches
/// no String
#[gtest]
#[tokio::test]
async fn o_qry_qlg_9045_a_string_only_resembling_a_time_value_compares_as_text() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let observation_id = namespace.urn("Observation", "resemblance")?;
    let observation_type = namespace.entity_type("Observation")?;
    let observation = Entity::builder()
        .id(observation_id.clone())
        .entity_type(observation_type)
        .property("readingTime", "12:00:00Z")
        .property("readingInstant", "2024-06-15T12:00:00Z")
        .property("readingDate", "2024-06-15")
        .property("readingLabel", "2024-01-01T01:00:00+01:00")
        .property("readingNote", "12:00:00 in the morning")
        .build()?;
    broker.entities().create(&observation).send().await?.done()?;
    let table = [
        (r#"readingNote=="12:00:00 in the morning""#, 1),
        (r#"readingNote!="09:30:00Z""#, 1),
        (r#"readingLabel=="2024-01-01T00:00:00Z""#, 0),
        ("readingLabel==2024-01-01T00:00:00Z", 0),
        (r#"readingLabel>"2023-01-01T00:00:00Z""#, 1),
        (r#"readingLabel<"2025-01-01T00:00:00Z""#, 1),
    ];
    let counts = observed(&broker, &observation_id, &table).await?;
    expect_that!(counts, eq(&table.to_vec()));
    Ok(())
}
