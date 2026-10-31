//! The emitted pointers followed unchanged walk the whole result.

use andromeda_test_suite::{broker, identity_matcher::ids_in_order, namespace::Namespace, paged_buildings::PagedBuildings};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::NgsiLdMediaType,
    query::{Locality, q},
};
use std::slice;

/// Pins: Query Entities following the emitted next, prev and first pointers unchanged pages through the whole result
/// Clause: 5.5.9.1, 6.3.10
#[gtest]
#[tokio::test]
async fn a_qry_pag_9089_the_emitted_pointers_followed_unchanged_walk_the_whole_result() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let buildings = PagedBuildings::create(&broker, &namespace).await?;
    let json_ld = broker.with_accept(Accept::new(NgsiLdMediaType::JsonLd));
    let mut page = json_ld
        .entities()
        .query()
        .entity_type(&buildings.entity_type)
        .locality(Locality::Local)
        .limit(1)
        .q(q::attr("height").gt(5))
        .send()
        .await?;
    expect_that!(page.items(), ids_in_order(&buildings.ids[..1]));
    for id in &buildings.ids[1..] {
        page = json_ld.send(&page.next_page().or_fail()?).await?;
        expect_that!(page.items(), ids_in_order(slice::from_ref(id)));
    }
    expect_that!(page.next(), none());
    let previous = json_ld.send(&page.prev_page().or_fail()?).await?;
    expect_that!(previous.items(), ids_in_order(&buildings.ids[2..3]));
    let first = json_ld.send(&previous.first_page().or_fail()?).await?;
    expect_that!(first.items(), ids_in_order(&buildings.ids[..1]));
    Ok(())
}
