//! Short names beside a full iri in attrs expand against the user context.

use andromeda_test_suite::{broker, hosted_context::HostedContext, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Upserted};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::QueryMethod},
    model::{Context, ContextEntry, InlineContext, NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::temporal_query,
};

/// Pins: Query Temporal Evolution of Entities with an attrs list naming one attribute by its full IRI and another
/// by its short name expands the short name against the user context of the Link header, so the entity holding the
/// same short name in the default vocabulary is not selected, through the GET parameter and the POST member
/// Clause: 5.5.7, 5.7.4.4, 6.18.3.2, 6.24.3.1
/// Tenant: private (its queries select by attributes, q or geometry alone, so the broker reads every entity and
/// forwards to every registration of the tenant)
#[gtest]
#[tokio::test]
async fn n_tmp_qry_3081_short_names_beside_a_full_iri_in_attrs_expand_against_the_user_context() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::private_tenant(&broker::front()?, &namespace).await?;
    let meter_a_id = namespace.urn("Meter", "a")?;
    let meter_type = namespace.entity_type("Meter")?;
    let meter_b_id = namespace.urn("Meter", "b")?;
    let meter_c_id = namespace.urn("Meter", "c")?;
    let context = Context::Single(ContextEntry::Inline(format!(r#"{{"Meter_{namespace}": "https://test.andromeda/{namespace}/Meter", "power": "https://test.andromeda/{namespace}/power", "meterState": "https://test.andromeda/{namespace}/meterState"}}"#).parse::<InlineContext>()?));
    let ctx = HostedContext::add(&broker, &context).await?;
    // entity a holds meterState in the default vocabulary: no Link header, so no user context applies
    let meter_a_evolution = TemporalEntity::builder()
        .id(meter_a_id)
        .entity_type(meter_type.clone())
        .property(
            "meterState",
            Property::builder("default vocabulary").observed_at("2020-01-01T00:00:00Z".parse()?),
        )
        .build()?;
    broker.temporal().create_or_update(&meter_a_evolution).send().await?.created()?;
    let linked = broker.with_context(Context::Single(ctx.entry()?))?;
    let meter_b_evolution = TemporalEntity::builder()
        .id(meter_b_id.clone())
        .entity_type(meter_type.clone())
        .property("meterState", Property::builder("user context").observed_at("2020-01-01T00:00:00Z".parse()?))
        .property("power", Property::builder(1).observed_at("2020-01-01T00:00:00Z".parse()?))
        .build()?;
    linked.temporal().create_or_update(&meter_b_evolution).send().await?.created()?;
    let meter_c_evolution = TemporalEntity::builder()
        .id(meter_c_id.clone())
        .entity_type(meter_type.clone())
        .property("power", Property::builder(2).observed_at("2020-01-01T00:00:00Z".parse()?))
        .build()?;
    linked.temporal().create_or_update(&meter_c_evolution).send().await?.created()?;
    // 5.5.7: the short name expands against the request's @context, as the full IRI is taken as it is
    let page = linked
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2000-01-01T00:00:00Z".parse()?))
        .attrs(vec!["meterState".parse()?])
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&meter_b_id)));
    let page_2 = linked
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2000-01-01T00:00:00Z".parse()?))
        .attrs(vec![format!("https://test.andromeda/{namespace}/power").parse()?, "meterState".parse()?])
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(2)));
    expect_that!(page_2.items(), contains(has_id(&meter_b_id)));
    expect_that!(page_2.items(), contains(has_id(&meter_c_id)));
    let page_3 = linked
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2000-01-01T00:00:00Z".parse()?))
        .method(QueryMethod::Post)
        .attrs(vec![format!("https://test.andromeda/{namespace}/power").parse()?, "meterState".parse()?])
        .send()
        .await?;
    expect_that!(page_3.items(), len(eq(2)));
    expect_that!(page_3.items(), contains(has_id(&meter_b_id)));
    expect_that!(page_3.items(), contains(has_id(&meter_c_id)));
    Ok(())
}
