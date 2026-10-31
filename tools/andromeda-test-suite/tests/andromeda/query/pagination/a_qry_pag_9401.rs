//! A cursor followed with entityMap added mints a map of every match.

use andromeda_test_suite::{
    broker,
    followed_link::request_with,
    identity_matcher::ids_in_order,
    namespace::Namespace,
    paged_buildings::PagedBuildings,
    unchecked_page::entities,
};
use googletest::prelude::*;
use http::{HeaderName, HeaderValue, StatusCode, header::ACCEPT};
use ngsi_ld::{
    http::{
        ResponseHead,
        headers::{Accept, Link, NgsiLdEntityMap, RelationType},
    },
    model::NgsiLdMediaType,
};

/// Pins: Query Entities following a next pointer with `entityMap=true` added answers 201 with an `NGSILD-EntityMap`
/// header and the following page, and the map holds every entity the query matches, also when `entityMapLifetime`
/// is added beside it
/// Clause: 4.21, 5.5.9.1, 5.5.14, 6.3.10
#[gtest]
#[tokio::test]
async fn a_qry_pag_9401_a_cursor_followed_with_entity_map_added_mints_a_map_of_every_match() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let buildings = PagedBuildings::create(&broker, &namespace).await?;
    let first = buildings.first_page(&broker, "2", &[("q", "height>5")]).await?;
    expect_that!(first.status(), eq(StatusCode::OK));
    expect_that!(entities(&first)?.as_slice(), ids_in_order(&buildings.ids[0..2]));
    expect_that!(first.headers().get(HeaderName::from_static("ngsild-entitymap")), none());
    let link = ResponseHead::new(first.headers().clone()).typed::<Link>().into_valid().or_fail()?;
    for parameters in [&[("entityMap", "true")][..], &[("entityMap", "true"), ("entityMapLifetime", "PT1H")]] {
        let followed = request_with(&broker, &link, &RelationType::Next, parameters)?.append_header(ACCEPT, HeaderValue::from_static("application/ld+json"));
        let page = broker.send_unchecked(followed).await?;
        expect_that!(page.status(), eq(StatusCode::CREATED));
        expect_that!(entities(&page)?.as_slice(), ids_in_order(&buildings.ids[2..4]));
        let map_id = ResponseHead::new(page.headers().clone())
            .typed::<NgsiLdEntityMap>()
            .into_valid()
            .and_then(|header| header.id())
            .or_fail()?;
        let map = broker
            .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
            .entity_maps()
            .retrieve(&map_id)
            .send()
            .await?;
        // Clause 5.5.14: the map fixes every entity the query matched, the first page's included.
        expect_that!(&map.entity_map, len(eq(buildings.ids.len())));
        for id in &buildings.ids {
            expect_that!(map.entity_map.get(id), some(anything()));
        }
    }
    Ok(())
}
