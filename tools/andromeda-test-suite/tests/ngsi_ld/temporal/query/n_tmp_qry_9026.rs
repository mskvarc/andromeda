//! A posted temporal query points at its neighbouring pages.

use andromeda_test_suite::{broker, identity_matcher::has_id, link_matcher::link_value, namespace::Namespace, outcome::Upserted};
use googletest::prelude::*;
use ngsi_ld::{
    http::{
        headers::{Accept, Link, RelationType},
        operation::{QueryMethod, ResultsCount},
    },
    model::{NgsiLdMediaType, Property, Value, evolution::TemporalEntity},
    query::temporal_query,
};

/// Pins: Query Temporal Evolution of Entities through POST `/temporal/entityOperations/query` as application/json
/// with limit 2 over four Entities answers two of them, NGSILD-Results-Count 4, a next Link of type application/json
/// and no prev Link; the next page answers the other two with a prev Link of that type and no next Link, its prev page
/// answers the first two again, and the same body posted with offset 2 answers the last two with a prev Link and no
/// next Link
/// Clause: 5.5.9.1, 5.7.4.4, 6.3.10, 6.24.3.1
#[gtest]
#[tokio::test]
async fn n_tmp_qry_9026_a_posted_temporal_query_points_at_its_neighbouring_pages() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let ids = vec![
        namespace.urn("Vehicle", 1)?,
        namespace.urn("Vehicle", 2)?,
        namespace.urn("Vehicle", 3)?,
        namespace.urn("Vehicle", 4)?,
    ];
    for id in &ids {
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
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let first = json
        .with_heads()
        .temporal()
        .query(temporal_query::before("2099-01-01T00:00:00Z".parse()?))
        .method(QueryMethod::Post)
        .entity_type(&vehicle_type)
        .ids(ids.clone())
        .limit(2)
        .count(ResultsCount::Include)
        .send()
        .await?;
    expect_that!(
        first.head.typed::<Link>().valid(),
        some(link_value(RelationType::Next).with_media_type("application/json"))
    );
    expect_that!(first.outcome.items(), len(eq(2)));
    expect_that!(first.outcome.total(), some(eq(4)));
    expect_that!(first.outcome.prev(), none());
    let second = json.with_heads().send(&first.outcome.next_page().or_fail()?).await?;
    expect_that!(
        second.head.typed::<Link>().valid(),
        some(link_value(RelationType::Prev).with_media_type("application/json"))
    );
    expect_that!(second.outcome.next(), none());
    let walked = first.outcome.items().iter().chain(second.outcome.items()).cloned().collect::<Vec<_>>();
    expect_that!(
        walked.as_slice(),
        unordered_elements_are![has_id(&ids[0]), has_id(&ids[1]), has_id(&ids[2]), has_id(&ids[3])]
    );
    let back = json.send(&second.outcome.prev_page().or_fail()?).await?;
    expect_that!(
        back.items(),
        unordered_elements_are![has_id(&first.outcome.items()[0].id), has_id(&first.outcome.items()[1].id)]
    );
    let offset = json
        .temporal()
        .query(temporal_query::before("2099-01-01T00:00:00Z".parse()?))
        .method(QueryMethod::Post)
        .entity_type(&vehicle_type)
        .ids(ids.clone())
        .limit(2)
        .offset(2)
        .send()
        .await?;
    expect_that!(offset.items(), len(eq(2)));
    expect_that!(offset.prev(), some(anything()));
    expect_that!(offset.next(), none());
    Ok(())
}
