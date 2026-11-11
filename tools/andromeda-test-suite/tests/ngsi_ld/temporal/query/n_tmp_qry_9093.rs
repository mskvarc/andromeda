//! A posted temporal query of two id patterns pages through their union alone.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, outcome::Upserted};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::QueryMethod},
    model::{NgsiLdMediaType, Property, Value, evolution::TemporalEntity},
    query::{EntitySelector, builders::SelectorTypeBuilder, temporal_query},
};

/// Pins: Query Temporal Evolution of Entities through POST `/temporal/entityOperations/query` with two
/// `EntitySelector`s of one type, one per `idPattern`, and limit 2 over four matches, two per pattern, beside a fifth
/// Entity of the type that neither pattern matches, answers two matches and a next Link whose page answers the other
/// two and no next Link
/// Clause: 5.5.9.1, 5.7.4.4, 5.2.33, 6.3.10, 6.24.3.1
#[gtest]
#[tokio::test]
async fn n_tmp_qry_9093_a_posted_temporal_query_of_two_id_patterns_pages_through_their_union_alone() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let matches = [
        namespace.urn("Vehicle", "north-1")?,
        namespace.urn("Vehicle", "north-2")?,
        namespace.urn("Vehicle", "south-1")?,
        namespace.urn("Vehicle", "south-2")?,
    ];
    let outsider_id = namespace.urn("Vehicle", "west-1")?;
    for id in matches.iter().chain([&outsider_id]) {
        let evolution = TemporalEntity::builder()
            .id(id.clone())
            .entity_type(vehicle_type.clone())
            .property(
                "speed",
                Property::builder(Value::Number("1.0".parse()?)).observed_at("2024-01-01T00:00:00Z".parse()?),
            )
            .build()?;
        broker.temporal().create_or_update(&evolution).send().await?.created()?;
    }
    let selectors = [
        EntitySelector::new(SelectorTypeBuilder::from(&vehicle_type).build()?).with_id_pattern(namespace.id_pattern_ending("Vehicle", "north-.*")?),
        EntitySelector::new(SelectorTypeBuilder::from(&vehicle_type).build()?).with_id_pattern(namespace.id_pattern_ending("Vehicle", "south-.*")?),
    ];
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let first = json
        .temporal()
        .query(temporal_query::before("2099-01-01T00:00:00Z".parse()?))
        .method(QueryMethod::Post)
        .selectors(selectors)
        .limit(2)
        .send()
        .await?;
    expect_that!(first.items(), len(eq(2)));
    let second = json.send(&first.next_page().or_fail()?).await?;
    expect_that!(second.next(), none());
    let walked = first.items().iter().chain(second.items()).cloned().collect::<Vec<_>>();
    expect_that!(
        walked.as_slice(),
        unordered_elements_are![has_id(&matches[0]), has_id(&matches[1]), has_id(&matches[2]), has_id(&matches[3])]
    );
    Ok(())
}
