//! An undeclared date string compares as text.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, q_counts::observed};
use googletest::prelude::*;
use ngsi_ld::model::Entity;

/// Pins: Query Entities with q comparing a Date-shaped string Property value with no `valueType` by ==, !=, >, >=, < and
/// <= selects the Entity where the quoted operand compares true as text, and for a bare operand only by !=
/// Clause: 4.6.3, 4.9, C.6
/// Recorded: a Property value is a time value only when its `valueType` declares it one (annex C.6); this one declares
/// none, so it is a String. A quoted operand is a String compared as text (clause 4.9, IETF RFC 8259 section 8.3),
/// and a bare time operand is of another data type, so it matches no String with any operator but !=
#[gtest]
#[tokio::test]
async fn o_qry_qlg_9044_an_undeclared_date_string_compares_as_text() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let observation_id = namespace.urn("Observation", "date-operators")?;
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
        ("readingDate==2024-06-15", 0),
        (r#"readingDate=="2024-06-15""#, 1),
        ("readingDate==2024-06-16", 0),
        (r#"readingDate=="2024-06-16""#, 0),
        ("readingDate!=2024-06-16", 1),
        (r#"readingDate!="2024-06-16""#, 1),
        ("readingDate!=2024-06-15", 1),
        (r#"readingDate!="2024-06-15""#, 0),
        ("readingDate>2024-06-14", 0),
        (r#"readingDate>"2024-06-14""#, 1),
        ("readingDate>2024-06-15", 0),
        (r#"readingDate>"2024-06-15""#, 0),
        ("readingDate>=2024-06-15", 0),
        (r#"readingDate>="2024-06-15""#, 1),
        ("readingDate>=2024-06-16", 0),
        (r#"readingDate>="2024-06-16""#, 0),
        ("readingDate<2024-06-16", 0),
        (r#"readingDate<"2024-06-16""#, 1),
        ("readingDate<2024-06-15", 0),
        (r#"readingDate<"2024-06-15""#, 0),
        ("readingDate<=2024-06-15", 0),
        (r#"readingDate<="2024-06-15""#, 1),
        ("readingDate<=2024-06-14", 0),
        (r#"readingDate<="2024-06-14""#, 0),
    ];
    let counts = observed(&broker, &observation_id, &table).await?;
    expect_that!(counts, eq(&table.to_vec()));
    Ok(())
}
