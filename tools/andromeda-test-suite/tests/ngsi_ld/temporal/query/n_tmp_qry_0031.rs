//! A temporal scope query selects the entity and keeps every instance in the window.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    evolution_matcher::has_evolution,
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    outcome::Upserted,
    problem::answered,
    temporal_page_body::entities,
    written_temporal_query::get_parameters,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        DateTime,
        EntityId,
        NgsiLdMediaType,
        Output,
        Property,
        Scope,
        evolution::{ScopeInstance, TemporalEntity},
    },
    query::{scope_query::ScopeQuery, temporal_query},
};

/// Pins: Query Temporal Evolution of Entities with scopeQ selects a Vehicle whose Scope moved from /<ns>/Harbour to
/// /<ns>/Centre by any Scope it held in the window and answers all 16 speed instances in the window, those observed
/// before its first Scope included, while a scopeQ it never held selects nothing
/// Clause: 4.18, 4.19, 5.7.4.4
/// Recorded: the last step of clause 5.7.4.4 keeps the instances meeting the temporal, query and geoquery
/// restrictions; the closing sentence of the informative Annex C clause C.5.16 is not followed
#[gtest]
#[tokio::test]
async fn n_tmp_qry_0031_a_temporal_scope_query_selects_the_entity_and_keeps_every_instance_in_the_window() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_moved_id = namespace.urn("Vehicle", "moved")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let harbour = format!("/{namespace}/Harbour");
    let centre = format!("/{namespace}/Centre");
    let speeds = [
        (0, "2025-12-31T23:58:00Z"),
        (1, "2025-12-31T23:59:00Z"),
        (2, "2026-01-01T00:00:00Z"),
        (3, "2026-01-01T00:01:00Z"),
        (4, "2026-01-01T00:02:00Z"),
        (5, "2026-01-01T00:03:00Z"),
        (6, "2026-01-01T00:04:00Z"),
        (7, "2026-01-01T00:05:00Z"),
        (8, "2026-01-01T00:06:00Z"),
        (9, "2026-01-01T00:07:00Z"),
        (10, "2026-01-01T00:08:00Z"),
        (11, "2026-01-01T00:09:00Z"),
        (12, "2026-01-01T00:10:00Z"),
        (13, "2026-01-01T00:11:00Z"),
        (14, "2026-01-01T00:12:00Z"),
        (15, "2026-01-01T00:13:00Z"),
    ];
    let mut vehicle_moved = TemporalEntity::builder()
        .id(vehicle_moved_id.clone())
        .entity_type(vehicle_type.clone())
        .scope(ScopeInstance::new(harbour.parse::<Scope>()?).with_observed_at("2026-01-01T00:00:00Z".parse()?))
        .scope(ScopeInstance::new(centre.parse::<Scope>()?).with_observed_at("2026-01-01T00:07:00Z".parse()?))
        .scope(ScopeInstance::new(centre.parse::<Scope>()?).with_observed_at("2026-01-01T00:10:00Z".parse()?));
    for (value, observed_at) in speeds {
        vehicle_moved = vehicle_moved.property("speed", Property::builder(value).observed_at(observed_at.parse()?));
    }
    let vehicle_moved = vehicle_moved.build()?;
    broker.temporal().create_or_update(&vehicle_moved).send().await?.created()?;
    let first_speed = "2025-12-31T23:58:00Z".parse::<DateTime>()?;
    let last_speed = "2026-01-01T00:13:00Z".parse::<DateTime>()?;
    let every_scope = format!("/{namespace}/#");
    for scope_q in [None, Some(harbour.as_str()), Some(centre.as_str()), Some(every_scope.as_str())] {
        let page = broker
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .temporal()
            .query(temporal_query::between("2025-12-31T00:00:00Z".parse()?, "2026-01-02T00:00:00Z".parse()?))
            .entity_type(&vehicle_type)
            .attrs(vec!["speed".parse()?])
            .maybe_scope_q(scope_q.map(str::parse::<ScopeQuery>).transpose()?)
            .send()
            .await?;
        require_whole_history(page.items(), &vehicle_moved_id, first_speed, last_speed)?;
    }
    // The client writes the "or" of a scope query as `|`, so the comma form is sent as written.
    let either = format!("{harbour},{centre}");
    let answer = get_parameters(
        &broker,
        &[
            ("type", vehicle_type.as_str()),
            ("attrs", "speed"),
            ("timerel", "between"),
            ("timeAt", "2025-12-31T00:00:00Z"),
            ("endTimeAt", "2026-01-02T00:00:00Z"),
            ("scopeQ", &either),
        ],
        "application/json",
    )
    .await?;
    expect_that!(&answer, answered(StatusCode::OK));
    require_whole_history(&entities(answer.as_ref().or_fail()?)?, &vehicle_moved_id, first_speed, last_speed)?;
    let elsewhere = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::between("2025-12-31T00:00:00Z".parse()?, "2026-01-02T00:00:00Z".parse()?))
        .entity_type(&vehicle_type)
        .attrs(vec!["speed".parse()?])
        .scope_q(format!("/{namespace}/Elsewhere").as_str().parse::<ScopeQuery>()?)
        .send()
        .await?;
    expect_that!(elsewhere.items(), len(eq(0)));
    Ok(())
}

/// Requires `items` to be the one Vehicle `id` with all 16 speed instances, from `first` (value 0) to `last` (value
/// 15), whatever the scope query that selected it.
fn require_whole_history(items: &[TemporalEntity<Output>], id: &EntityId, first: DateTime, last: DateTime) -> Result<()> {
    verify_that!(items, len(eq(1)))?;
    verify_that!(items, nth(0, has_id(id)))?;
    verify_that!(
        items,
        nth(
            0,
            has_evolution(
                "speed",
                all!(
                    len(eq(16)),
                    nth(0, any_attribute().value(0).observed_at(first)),
                    nth(15, any_attribute().value(15).observed_at(last))
                )
            )
        )
    )
}
