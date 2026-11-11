//! A tenant over its posted-query quota evicts the oldest held query.

use andromeda_test_suite::{broker, config::BrokerRole, namespace::Namespace, paged_buildings::PagedBuildings, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::QueryMethod},
    model::{ErrorType, NgsiLdMediaType},
    query::{Locality, q},
};

/// Pins: On a broker holding at most two posted queries per Tenant, three Query Entities through POST
/// `/entityOperations/query` with limit 1 and differing `q`, each answering a next Link, leave the next Link of the
/// first answering 400 `BadRequestData` and the next Links of the second and third answering their next pages
/// Clause: 5.5.9.1, 6.3.10, 6.23.3.1
/// Tenant: private (the quota counts every posted query of the Tenant)
#[gtest]
#[tokio::test]
async fn a_qry_pag_9095_a_tenant_over_its_posted_query_quota_evicts_the_oldest_held_query() -> Result<()> {
    let namespace = Namespace::fresh();
    let durable = broker::client(BrokerRole::Durable)?;
    let tenant = broker::private_tenant(&durable, &namespace).await?;
    let buildings = PagedBuildings::create(&tenant, &namespace).await?;
    let json = tenant.with_accept(Accept::new(NgsiLdMediaType::Json));
    let mut pages = Vec::new();
    for threshold in [5, 15, 25] {
        let page = json
            .entities()
            .query()
            .method(QueryMethod::Post)
            .entity_type(&buildings.entity_type)
            .q(q::attr("height").gt(threshold))
            .locality(Locality::Local)
            .limit(1)
            .send()
            .await?;
        expect_that!(page.next(), some(anything()));
        pages.push(page);
    }
    let evicted = json.with_responses().send(&pages[0].next_page().or_fail()?).await;
    expect_that!(&evicted, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    for kept in &pages[1..] {
        let followed = json.send(&kept.next_page().or_fail()?).await?;
        expect_that!(followed.items(), len(eq(1)));
    }
    Ok(())
}
