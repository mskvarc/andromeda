//! An offset at or past the end answers an empty array with only prev.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::ResultsCount},
    model::{Entity, NgsiLdMediaType},
    query::Locality,
};

/// Pins: Query Entities with limit 2 over two entities and an offset of 2 or 3 answers 200 with an empty array,
/// `NGSILD-Results-Count` 2 and a prev pointer alone
/// Clause: 5.5.9.2, 6.3.10, Table 6.3.10-2
/// Recorded: Table 6.3.10-2 makes the too-large offset a "should" and its rule sets the offset to the size of the result
/// set, which selects nothing; Andromeda answers 200 with an empty array
#[gtest]
#[tokio::test]
async fn o_qry_pag_9017_an_offset_at_or_past_the_end_answers_an_empty_array_with_only_prev() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_type = namespace.entity_type("Building")?;
    for (n, name) in ["b0", "b1"].into_iter().enumerate() {
        let building = Entity::builder()
            .id(namespace.urn("Building", format!("{n:04}"))?)
            .entity_type(building_type.clone())
            .property("name", name)
            .build()?;
        broker.entities().create(&building).send().await?.done()?;
    }
    for offset in [2, 3] {
        let page = broker
            .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
            .entities()
            .query()
            .entity_type(&building_type)
            .locality(Locality::Local)
            .limit(2)
            .count(ResultsCount::Include)
            .offset(offset)
            .send()
            .await?;
        expect_that!(page.items(), is_empty());
        expect_that!(page.total(), some(eq(2)));
        expect_that!(page.prev(), some(anything()));
        expect_that!(page.next(), none());
        expect_that!(page.first(), none());
        expect_that!(page.last(), none());
    }
    Ok(())
}
