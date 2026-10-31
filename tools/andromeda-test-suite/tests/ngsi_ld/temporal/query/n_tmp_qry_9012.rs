//! A scope query selects entities without narrowing their attributes.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    evolution_matcher::has_evolution,
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType, Property, Value},
    query::{scope_query::ScopeQuery, temporal_query},
};

/// Pins: Query Temporal Evolution of Entities with a `scopeQ` the Entity scope matches and a window on the default
/// `observedAt` answers the Entity with the instance of its temperature observed inside the window
/// Clause: 4.19, 5.7.4.4
#[gtest]
#[tokio::test]
async fn n_tmp_qry_9012_a_scope_query_selects_entities_without_narrowing_their_attributes() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let building_scoped_id = namespace.urn("Building", "scoped")?;
    let building_type = namespace.entity_type("Building")?;
    let building_scoped = Entity::builder()
        .id(building_scoped_id.clone())
        .entity_type(building_type.clone())
        .scope(format!("/{namespace}").as_str())
        .property(
            "temperature",
            Property::builder(Value::Number("21.5".parse()?)).observed_at("2025-01-01T00:00:00Z".parse()?),
        )
        .build()?;
    broker.entities().create(&building_scoped).send().await?.done()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2024-01-01T00:00:00Z".parse()?))
        .entity_type(&building_type)
        .scope_q(format!("/{namespace}").as_str().parse::<ScopeQuery>()?)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(
        page.items(),
        nth(
            0,
            all!(
                has_id(&building_scoped_id),
                has_evolution(
                    "temperature",
                    all!(
                        len(eq(1)),
                        nth(
                            0,
                            any_attribute()
                                .value(Value::Number("21.5".parse()?))
                                .observed_at("2025-01-01T00:00:00Z".parse()?)
                        )
                    )
                )
            )
        )
    );
    Ok(())
}
