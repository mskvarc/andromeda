//! A temporal query counts and pages only the entities a projection leaves a member.

use andromeda_test_suite::{
    broker::{self, BrokerClient},
    namespace::Namespace,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{Page, headers::Accept, operation::ResultsCount},
    model::{Entity, EntityTypeName, NgsiLdMediaType, Output, Projected, Property, evolution::TemporalEntity},
    query::{
        projection::Projection,
        temporal_query::{self, TemporalProperty},
    },
};

/// How a page of the Buildings is projected.
enum Projecting {
    /// Keeps the members of the projection.
    Pick(&'static str),
    /// Removes the members of the projection.
    Omit(&'static str),
}

/// The page of one Building at `offset`, counted, under `projecting`.
async fn page_at(
    client: &BrokerClient,
    building_type: &EntityTypeName,
    projecting: &Projecting,
    offset: u32,
) -> Result<Page<TemporalEntity<Output, Projected>>> {
    let temporal = client.temporal();
    let query = temporal
        .query_projected(temporal_query::after("2000-01-01T00:00:00Z".parse()?).time_property(TemporalProperty::ModifiedAt))
        .entity_type(building_type);
    Ok(match projecting {
        Projecting::Pick(members) => {
            query
                .pick(members.parse::<Projection>()?)
                .count(ResultsCount::Include)
                .limit(1)
                .offset(offset)
                .send()
                .await?
        }
        Projecting::Omit(members) => {
            query
                .omit(members.parse::<Projection>()?)
                .count(ResultsCount::Include)
                .limit(1)
                .offset(offset)
                .send()
                .await?
        }
    })
}

/// Pins: Query Temporal Evolution of Entities over three Buildings, one without name, with count and limit 1 serves
/// three pages counted 3 under pick=id,name, and two pages counted 2 and an empty third page under pick=name and
/// under omit=id,type,category
/// Clause: 4.21, 5.5.9, 5.7.4.4, 6.3.13
#[gtest]
#[tokio::test]
async fn n_tmp_qry_9010_a_temporal_query_counts_and_pages_only_the_entities_a_projection_leaves_a_member() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_type = namespace.entity_type("Building")?;
    let emptied = [("emptied-a", "name"), ("emptied-b", "category"), ("emptied-c", "name")];
    for (suffix, attribute) in emptied {
        let building = Entity::builder()
            .id(namespace.urn("Building", suffix)?)
            .entity_type(building_type.clone())
            .property(attribute, Property::builder("Depot"))
            .build()?;
        broker.entities().create(&building).send().await?.done()?;
    }
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let picking_ids = Projecting::Pick("id,name");
    let mut picked = Vec::new();
    for offset in 0..3 {
        let page = page_at(&json, &building_type, &picking_ids, offset).await?;
        expect_that!(page.total(), some(eq(3)));
        expect_that!(page.items(), len(eq(1)));
        let id = page.items().first().and_then(|entity| entity.id.clone()).or_fail()?;
        expect_that!(picked.contains(&id), eq(false));
        picked.push(id);
    }
    let beyond = page_at(&json, &building_type, &picking_ids, 3).await?;
    expect_that!(beyond.total(), some(eq(3)));
    expect_that!(beyond.items(), is_empty());
    for projecting in [Projecting::Pick("name"), Projecting::Omit("id,type,category")] {
        for offset in 0..2 {
            let page = page_at(&json, &building_type, &projecting, offset).await?;
            expect_that!(page.total(), some(eq(2)));
            expect_that!(page.items(), len(eq(1)));
            expect_that!(page.items().first().map(|entity| entity.id.is_none()), some(eq(true)));
        }
        let third = page_at(&json, &building_type, &projecting, 2).await?;
        expect_that!(third.total(), some(eq(2)));
        expect_that!(third.items(), is_empty());
    }
    Ok(())
}
