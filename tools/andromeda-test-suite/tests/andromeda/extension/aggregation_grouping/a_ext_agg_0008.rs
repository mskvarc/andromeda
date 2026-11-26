//! A value key named by a quoted IRI holding a comma or a semicolon is one key.

use andromeda_test_suite::{
    broker,
    grouped_aggregation::{GroupedAttribute, GroupedEntity, grouped_request},
    hosted_context::HostedContext,
    namespace::Namespace,
    outcome::Upserted,
    problem::answered,
};
use cefact_units::UnitCode;
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::model::{Context, ContextEntry, InlineContext, Property, evolution::TemporalEntity};

/// Pins: a grouped temporal retrieve with no @context and aggrGroupBy="urn:x:<namespace>:zone;(a,b)", the attribute an
/// @context maps zone to, answers 200 grouping each speed instance by the value zone had in effect at its
/// `observedAt`, the `;` and the `,` the IRI holds being part of the key
/// Clause: 4.5.19, 4.9, 5.5.7, 6.19.3.1
/// Recorded: Andromeda extension, not v1.9.1: the temporal binding defines neither `aggrGroupBy` nor the
/// `groupedValues` format; the `temporal_aggregation_grouping` extension adds them, here with at most 3 groups, and
/// reads a key as a clause 4.9 value path whose absolute IRI is a `quotedStr`, splitting the keys and a key's bucket
/// only at a mark outside it
#[gtest]
#[tokio::test]
async fn a_ext_agg_0008_a_value_key_named_by_a_quoted_iri_holding_a_comma_or_a_semicolon_is_one_key() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let termed = HostedContext::add(
        &broker,
        &Context::Single(ContextEntry::Inline(
            format!(r#"{{"zone":"urn:x:{namespace}:zone;(a,b)"}}"#).parse::<InlineContext>()?,
        )),
    )
    .await?;
    let writer = broker.with_context(Context::Single(termed.entry()?))?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_evolution = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(namespace.entity_type("Vehicle")?)
        .property(
            "speed",
            Property::builder(30).unit_code(UnitCode::Kmh).observed_at("2024-01-15T08:30:00Z".parse()?),
        )
        .property(
            "speed",
            Property::builder(40).unit_code(UnitCode::Kmh).observed_at("2024-03-01T10:00:00Z".parse()?),
        )
        .property("zone", Property::builder("north").observed_at("2024-01-01T00:00:00Z".parse()?))
        .property("zone", Property::builder("south").observed_at("2024-02-15T00:00:00Z".parse()?))
        .build()?;
    writer.temporal().create_or_update(&vehicle_evolution).send().await?.created()?;
    // The client cannot write a parameter of this read, so it is sent as written, the quoted IRI percent-encoded.
    let request = grouped_request(
        &broker,
        &vehicle_id,
        &format!(
            "timerel=after&timeAt=1970-01-01T00%3A00%3A00Z&attrs=speed&format=groupedValues&aggrMethods=avg&aggrGroupBy=%22urn:x:{namespace}:zone%3B%28a%2Cb%29%22"
        ),
    )?;
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::OK));
    let grouped = GroupedEntity::read(answer.as_ref().or_fail()?)?;
    let series = grouped.attribute("speed").and_then(GroupedAttribute::single).or_fail()?;
    expect_that!(series.group_value("avg", "north"), some(eq(30.0)));
    expect_that!(series.group_value("avg", "south"), some(eq(40.0)));
    expect_that!(series.group_count("avg"), eq(2));
    Ok(())
}
