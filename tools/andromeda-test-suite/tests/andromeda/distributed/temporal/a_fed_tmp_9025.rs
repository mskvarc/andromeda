//! A grouping is applied over the merged instances of an entity held in several parts.

use andromeda_test_suite::{
    broker,
    context_source::ContextSource,
    forwarded_query::query_values,
    grouped_aggregation::GroupedAttribute,
    grouped_query::{grouped_entities, grouped_query_request},
    namespace::Namespace,
    outcome::Upserted,
    problem::{answered, problem},
    source_answer,
    source_request::SourceRequest,
};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode};
use ngsi_ld::{
    model::{EntityId, ErrorType, Input, Property, evolution::TemporalEntity},
    query::IdPattern,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo},
};

/// The path the source answers Query Temporal Evolution of Entities at.
const SOURCE_PATH: &str = "/a/ngsi-ld/v1/temporal/entities";

/// Pins: Query Temporal Evolution of Entities with splitEntities true as groupedValues avg and totalCount by
/// observedAt;month and a q over a Vehicle whose instances are two held by the broker (January, February) and two
/// held by the source (February, March) answers one group per month over the four merged instances, asks the source
/// for instances without grouping, and answers 403 `TooManyResults` when an April instance makes a fourth group over
/// the bound of 3; without a filter the source is asked for its plain aggregate, and the groups stay the ones built
/// over the broker's own instances
/// Clause: 5.7.4.4, 4.5.19, 5.5.2
/// Recorded: Andromeda extension, not v1.9.1: the temporal binding defines neither aggrGroupBy nor the groupedValues
/// format; the `temporal_aggregation_grouping` extension adds them, here with at most 3 groups. The grouping is
/// applied by the broker over the merged instances and is not forwarded
#[gtest]
#[tokio::test]
async fn a_fed_tmp_9025_a_grouping_is_applied_over_the_merged_instances_of_an_entity_held_in_several_parts() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_2_id = namespace.urn("Vehicle", 2)?;
    let registration_id = namespace.registration_id(1)?;
    let held = |id: EntityId| -> Result<TemporalEntity<Input>> {
        Ok(TemporalEntity::builder()
            .id(id)
            .entity_type(vehicle_type.clone())
            .property("speed", Property::builder(10).observed_at("2024-01-10T08:00:00Z".parse()?))
            .property("speed", Property::builder(20).observed_at("2024-02-10T08:00:00Z".parse()?))
            .build()?)
    };
    broker.temporal().create_or_update(&held(vehicle_id.clone())?).send().await?.created()?;
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(format!("urn:ngsi-ld:Vehicle:{namespace}-.*").parse::<IdPattern>()?)])
            .build()?])
        .operations([DistributedOperation::QueryTemporal])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // The source answers instances that carry an instanceId, which the model builds none of, so its answer is text.
    let source_answer_1 = format!(
        r#"[{{"id": "{vehicle_id}", "type": "{vehicle_type}", "speed": [
{{"type": "Property", "value": 30, "observedAt": "2024-02-20T08:00:00Z", "instanceId": "urn:ngsi-ld:instance:{namespace}-40"}},
{{"type": "Property", "value": 40, "observedAt": "2024-03-10T08:00:00Z", "instanceId": "urn:ngsi-ld:instance:{namespace}-41"}},
{{"type": "Property", "value": 50, "observedAt": "2024-04-10T08:00:00Z", "instanceId": "urn:ngsi-ld:instance:{namespace}-42"}}
]}}]"#
    );
    source
        .answer_when(
            SourceRequest::new(Method::GET, SOURCE_PATH),
            source_answer::text(StatusCode::OK, HeaderValue::from_static("application/json"), &source_answer_1),
        )
        .await?;
    let grouped = |parameters: &str| format!("type={vehicle_type}&splitEntities=true&format=groupedValues&aggrGroupBy=observedAt%3Bmonth{parameters}");
    let answer = broker
        .send_unchecked(grouped_query_request(
            &broker,
            &grouped("&timerel=between&timeAt=2024-01-01T00%3A00%3A00Z&endTimeAt=2024-04-01T00%3A00%3A00Z&aggrMethods=avg%2CtotalCount&q=speed%3E0"),
        )?)
        .await;
    expect_that!(&answer, answered(StatusCode::OK));
    let entities = grouped_entities(&answer?)?;
    expect_that!(entities.len(), eq(1));
    let speed = entities
        .first()
        .and_then(|entity| entity.attribute("speed"))
        .and_then(GroupedAttribute::single)
        .or_fail()?;
    expect_that!(speed.group_count("totalCount"), eq(3));
    expect_that!(speed.group_value("totalCount", "2024-01-01T00:00:00Z"), some(eq(1.0)));
    expect_that!(speed.group_value("totalCount", "2024-02-01T00:00:00Z"), some(eq(2.0)));
    expect_that!(speed.group_value("totalCount", "2024-03-01T00:00:00Z"), some(eq(1.0)));
    expect_that!(speed.group_value("avg", "2024-01-01T00:00:00Z"), some(eq(10.0)));
    expect_that!(speed.group_value("avg", "2024-02-01T00:00:00Z"), some(eq(25.0)));
    expect_that!(speed.group_value("avg", "2024-03-01T00:00:00Z"), some(eq(40.0)));
    let forwarded = source.wait_for_on(Method::GET, SOURCE_PATH, 1).await?;
    let first = forwarded.first().or_fail()?;
    expect_that!(query_values(first, "aggrGroupBy"), is_empty());
    expect_that!(query_values(first, "aggrMethods"), is_empty());
    expect_that!(query_values(first, "format"), is_empty());
    // An April instance makes a fourth group over the bound of 3.
    let refused = broker
        .send_unchecked(grouped_query_request(
            &broker,
            &grouped("&timerel=after&timeAt=2024-01-01T00%3A00%3A00Z&aggrMethods=avg&q=speed%3E0"),
        )?)
        .await;
    expect_that!(&refused, problem(StatusCode::FORBIDDEN, ErrorType::TooManyResults));
    broker.temporal().create_or_update(&held(vehicle_2_id.clone())?).send().await?.created()?;
    // Without a filter the source is asked for its plain aggregate, which the broker leaves out of the groups.
    let source_answer_2 = format!(
        r#"[{{"id": "{vehicle_2_id}", "type": "{vehicle_type}", "speed": {{"type": "Property", "avg": [[100, "2024-01-01T00:00:00Z", "2024-04-01T00:00:00Z"]]}}}}]"#
    );
    source
        .answer_when(
            SourceRequest::new(Method::GET, SOURCE_PATH).query_containing("format", "").priority(1),
            source_answer::text(StatusCode::OK, HeaderValue::from_static("application/json"), &source_answer_2),
        )
        .await?;
    let answer_3 = broker
        .send_unchecked(grouped_query_request(
            &broker,
            &format!(
                "{}&id=urn%3Angsi-ld%3AVehicle%3A{namespace}-2&timerel=between&timeAt=2024-01-01T00%3A00%3A00Z&endTimeAt=2024-04-01T00%3A00%3A00Z&aggrMethods=avg",
                grouped("")
            ),
        )?)
        .await;
    expect_that!(&answer_3, answered(StatusCode::OK));
    let entities_2 = grouped_entities(&answer_3?)?;
    expect_that!(entities_2.len(), eq(1));
    let speed_2 = entities_2
        .first()
        .and_then(|entity| entity.attribute("speed"))
        .and_then(GroupedAttribute::single)
        .or_fail()?;
    expect_that!(speed_2.group_count("avg"), eq(2));
    expect_that!(speed_2.group_value("avg", "2024-01-01T00:00:00Z"), some(eq(10.0)));
    expect_that!(speed_2.group_value("avg", "2024-02-01T00:00:00Z"), some(eq(20.0)));
    for request in source.received_on(Method::GET, SOURCE_PATH).await? {
        if query_values(&request, "id").first() == Some(&vehicle_2_id.to_string()) {
            expect_that!(query_values(&request, "aggrGroupBy"), is_empty());
        }
    }
    broker.registrations().delete(&registration_id).send().await?;
    Ok(())
}
