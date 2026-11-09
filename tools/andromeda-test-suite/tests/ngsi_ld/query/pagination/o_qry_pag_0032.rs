//! A counted cursor page without local counts the whole collection.

use andromeda_test_suite::{
    broker,
    followed_link::request,
    identity_matcher::ids_in_order,
    namespace::Namespace,
    outcome::Done,
    problem::answered,
    raw_call::target,
    unchecked_call::exchange,
    unchecked_page::entities,
};
use googletest::prelude::*;
use http::{Method, StatusCode, Uri};
use ngsi_ld::{
    http::{
        ResponseHead,
        headers::{Link, RelationType},
    },
    model::Entity,
};

/// Pins: Query Entities without local, started with an empty cursor and followed by its next pointer with count true
/// added, over five entities, answers the third and fourth entities and NGSILD-Results-Count 5
/// Clause: 6.3.13, 5.5.9.1, 5.5.9.2, 6.3.10
/// Tenant: private (a query without local walks by cursor only while no registration of the tenant can contribute
/// to it; in the shared tenant a registration of another file naming no entity type matches every type (clause
/// 5.12) and makes the query federated; clause 4.14 keeps each tenant's registrations its own)
/// Recorded: 5.5.9.2 lets the pointers of 5.5.9.1 be "implemented in a completely opaque way"; Andromeda walks a
/// query in its default order by an opaque cursor once the request sends an empty `cursor`, and offers first and last
/// pointers beside next and prev
/// Recorded: no order is fixed for a query without `orderBy`; Andromeda pages in ascending identifier order
#[gtest]
#[tokio::test]
async fn o_qry_pag_0032_a_counted_cursor_page_without_local_counts_the_whole_collection() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::private_tenant(&broker::front()?, &namespace).await?;
    let building_type = namespace.entity_type("Building")?;
    let mut ids = Vec::new();
    for n in 0..5 {
        let id = namespace.urn("Building", format!("{n:04}"))?;
        let building = Entity::builder()
            .id(id.clone())
            .entity_type(building_type.clone())
            .property("name", format!("b{n}"))
            .build()?;
        broker.entities().create(&building).send().await?.done()?;
        ids.push(id);
    }
    let type_text = building_type.to_string();
    // The walk starts with an empty `cursor`, which no operation writes, and a page is followed through the Link field
    // of the answer it came in, where the typed page request takes no further parameter, so the first read and the
    // followed one go out as written.
    let read = target("entities", &[("type", type_text.as_str()), ("limit", "2"), ("cursor", "")]);
    let first = broker
        .send_unchecked(exchange(&broker, Method::GET, &read, &[("Accept", "application/ld+json")], None)?)
        .await;
    expect_that!(&first, answered(StatusCode::OK));
    let first = first?;
    let head = ResponseHead::new(first.headers().clone());
    let typed = head.typed::<Link>();
    let link = typed.valid().or_fail()?;
    expect_that!(link.find_by_rel(&RelationType::Next).next(), some(anything()));
    let followed = request(&broker, link, &RelationType::Next)?;
    let counted = format!("{}&count=true", followed.as_request().uri()).parse::<Uri>()?;
    let second = broker.send_unchecked(followed.uri(counted)).await;
    expect_that!(&second, answered(StatusCode::OK));
    let second = second?;
    expect_that!(entities(&second)?.as_slice(), ids_in_order(&ids[2..4]));
    expect_that!(
        second.headers().get("ngsild-results-count").and_then(|value| value.to_str().ok()),
        some(eq("5"))
    );
    Ok(())
}
