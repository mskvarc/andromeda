//! Pick and omit apply both and a member in both is refused by get and by post.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Upserted, problem::problem, projected_evolution::evolution_names};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::QueryMethod},
    model::{ErrorType, NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::{projection::Projection, temporal_query},
};

/// Pins: Query Temporal Evolution of Entities by GET and by POST with pick and omit naming different members answers 200
/// with each temporal Entity reduced to the picked members minus the omitted ones; a member named in both is refused
/// with `BadRequestData` on either binding
/// Clause: 4.21, 5.7.4.5, 5.2.23, 6.19.3.1
/// Recorded: The Query Temporal Evolution parameters and the Query data type define pick and omit side by side and the
/// clauses name no error for either combination. Applying both when they name different members, and refusing a member
/// named in both as a self-contradictory projection with `BadRequestData`, is Andromeda's reading, so the file is kind O.
/// Ruled by the user (2026-10-05; the refusal of a member named in both 2026-10-11)
#[gtest]
#[tokio::test]
async fn o_tmp_qry_4005_pick_and_omit_apply_both_and_a_member_in_both_is_refused_by_get_and_by_post() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let building_evolution = TemporalEntity::builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
        .property("category", Property::builder("commercial").observed_at("2020-08-01T12:03:00Z".parse()?))
        .property("name", Property::builder("Depot").observed_at("2020-08-01T12:03:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&building_evolution).send().await?.created()?;
    for method in [QueryMethod::Get, QueryMethod::Post] {
        let page = broker
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .temporal()
            .query_projected(temporal_query::after("2020-07-01T00:00:00Z".parse()?))
            .method(method)
            .entity_type(&building_type)
            .pick("id,category".parse::<Projection>()?)
            .omit("name".parse::<Projection>()?)
            .send()
            .await?;
        expect_that!(page.items(), len(eq(1)));
        let entity = page.items().first().or_fail()?;
        expect_that!(entity.id.as_ref(), some(eq(&building_id)));
        let names = evolution_names(entity);
        expect_that!(&names, contains(eq("category")));
        expect_that!(&names, not(contains(eq("name"))));
        let answer = broker
            .with_responses()
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .temporal()
            .query_projected(temporal_query::after("2020-07-01T00:00:00Z".parse()?))
            .method(method)
            .entity_type(&building_type)
            .pick("id,category,name".parse::<Projection>()?)
            .omit("name".parse::<Projection>()?)
            .send()
            .await;
        expect_that!(&answer, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    }
    Ok(())
}
