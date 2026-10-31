//! Every value datatype admits both counting methods.

use andromeda_test_suite::{
    aggregation_matcher::{has_aggregate, period},
    body_json::{json_count, json_equals},
    broker,
    namespace::Namespace,
    nth::nth,
    problem::answered,
    raw_temporal,
    unchecked_document::from_rendered,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::CreateOrUpdateTemporalEvolution},
    model::{
        Date,
        DateTime,
        JsonProperty,
        Lossless,
        NgsiLdMediaType,
        Property,
        RawJson,
        Relationship,
        Time,
        Value,
        evolution::{AggregationMethod, TemporalEntity},
    },
    query::temporal_query,
};

/// Pins: `distinctCount` and `totalCount` over two distinct observations of a Number, String, Boolean, JSON Object,
/// JSON Array, `DateTime`, Date and Time Property and a Relationship each answer 2
/// Clause: Tables 4.5.19.1-1 to 4.5.19.1-3, 5.7.3.4
#[gtest]
#[tokio::test]
async fn n_tmp_agg_9006_every_value_datatype_admits_both_counting_methods() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let subject_id = namespace.urn("AggregationSubject", "counting")?;
    let subject_type = namespace.entity_type("AggregationSubject")?;
    let first = "2024-01-01T00:00:00Z".parse::<DateTime>()?;
    let second = "2024-01-01T01:00:00Z".parse::<DateTime>()?;
    let subject = TemporalEntity::builder()
        .id(subject_id.clone())
        .entity_type(subject_type)
        .property(namespace.attribute("speed")?, Property::builder(10).observed_at(first))
        .property(namespace.attribute("speed")?, Property::builder(20).observed_at(second))
        .property(namespace.attribute("brandName")?, Property::builder("alpha").observed_at(first))
        .property(namespace.attribute("brandName")?, Property::builder("beta").observed_at(second))
        .property(namespace.attribute("isParked")?, Property::builder(true).observed_at(first))
        .property(namespace.attribute("isParked")?, Property::builder(false).observed_at(second))
        .json_property(
            namespace.attribute("settings")?,
            JsonProperty::builder(r#"{"mode": "eco"}"#.parse::<RawJson>()?).observed_at(first),
        )
        .json_property(
            namespace.attribute("settings")?,
            JsonProperty::builder(r#"{"mode": "sport"}"#.parse::<RawJson>()?).observed_at(second),
        )
        .property(
            namespace.attribute("seenAt")?,
            Property::builder(Value::from("2024-03-01T05:00:00Z".parse::<DateTime>()?)).observed_at(first),
        )
        .property(
            namespace.attribute("seenAt")?,
            Property::builder(Value::from("2024-03-01T07:00:00Z".parse::<DateTime>()?)).observed_at(second),
        )
        .property(
            namespace.attribute("entryDate")?,
            Property::builder(Value::from("2024-03-01".parse::<Date>()?)).observed_at(first),
        )
        .property(
            namespace.attribute("entryDate")?,
            Property::builder(Value::from("2024-03-03".parse::<Date>()?)).observed_at(second),
        )
        .property(
            namespace.attribute("entryTime")?,
            Property::builder(Value::from("08:00:00Z".parse::<Time>()?)).observed_at(first),
        )
        .property(
            namespace.attribute("entryTime")?,
            Property::builder(Value::from("10:00:00Z".parse::<Time>()?)).observed_at(second),
        )
        .relationship(
            namespace.attribute("parkedAt")?,
            Relationship::builder(namespace.urn("OffStreetParking", 1)?).observed_at(first),
        )
        .relationship(
            namespace.attribute("parkedAt")?,
            Relationship::builder(namespace.urn("OffStreetParking", 2)?).observed_at(second),
        )
        .build()?;
    // The model writes no `@json` value object, so the `tags` evolution is set as text on the rendered document.
    let tags = namespace.attribute("tags")?;
    let tag_instances: RawJson = concat!(
        r#"[{"type": "Property", "value": {"@type": "@json", "@value": [1, 2]}, "observedAt": "2024-01-01T00:00:00Z"},"#,
        r#"{"type": "Property", "value": {"@type": "@json", "@value": [1, 2, 3, 4]}, "observedAt": "2024-01-01T01:00:00Z"}]"#
    )
    .parse()?;
    let document: UncheckedDocument = from_rendered(&subject.render(Lossless::Normalized))?.set_member(tags.as_ref(), &tag_instances);
    let request = broker
        .unchecked(&CreateOrUpdateTemporalEvolution::builder().entity(&subject).build())?
        .document(&document);
    let created = broker.send_unchecked(request).await;
    expect_that!(&created, answered(StatusCode::CREATED));
    for base in [
        "speed",
        "brandName",
        "isParked",
        "settings",
        "tags",
        "seenAt",
        "entryDate",
        "entryTime",
        "parkedAt",
    ] {
        let attribute = namespace.attribute(base)?;
        if base == "settings" {
            // The aggregated reader reads a Property or a Relationship only, so the answer for a JsonProperty is read as JSON.
            let answer = raw_temporal::retrieve(
                &broker,
                &subject_id,
                &[
                    ("timerel", "after"),
                    ("timeAt", "2023-12-31T00:00:00Z"),
                    ("attrs", attribute.as_str()),
                    ("format", "aggregatedValues"),
                    ("aggrMethods", "distinctCount,totalCount"),
                ],
            )
            .await?;
            expect_that!(answer.status(), eq(StatusCode::OK));
            for method in ["distinctCount", "totalCount"] {
                expect_that!(answer.body(), json_count(&format!("$.{attribute}.{method}"), 1));
                expect_that!(answer.body(), json_equals(&format!("$.{attribute}.{method}[0][0]"), 2));
            }
            continue;
        }
        let counted = broker
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .temporal()
            .retrieve_aggregated(&subject_id)
            .temporal_q(
                temporal_query::after("2023-12-31T00:00:00Z".parse()?).aggregation_methods([AggregationMethod::DistinctCount, AggregationMethod::TotalCount]),
            )
            .attrs(vec![attribute.to_string().parse()?])
            .send()
            .await?;
        for method in [AggregationMethod::DistinctCount, AggregationMethod::TotalCount] {
            expect_that!(
                counted.body(),
                has_aggregate(attribute.as_str(), method, all!(len(eq(1)), nth(0, period().value(2))))
            );
        }
    }
    Ok(())
}
