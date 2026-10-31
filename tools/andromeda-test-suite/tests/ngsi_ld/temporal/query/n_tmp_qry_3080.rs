//! Attrs naming a full iri selects the entity under a user context.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    evolution_matcher::{has_evolution, lacks_evolution},
    hosted_context::HostedContext,
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    outcome::Upserted,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{ContextDelivery, headers::Accept, operation::QueryMethod},
    model::{Context, ContextEntry, InlineContext, NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::temporal_query,
};

/// Pins: Query Temporal Evolution of Entities with attrs as the only selector selects and projects by an attribute
/// named with its full IRI or a compact IRI, under a Link header carrying a user context, through the GET parameter
/// and the POST member
/// Clause: 5.5.7, 5.7.4.4, 6.18.3.2, 6.24.3.1
/// Tenant: private (its queries select by attributes, q or geometry alone, so the broker reads every entity and
/// forwards to every registration of the tenant)
#[gtest]
#[tokio::test]
async fn n_tmp_qry_3080_attrs_naming_a_full_iri_selects_the_entity_under_a_user_context() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::private_tenant(&broker::front()?, &namespace).await?;
    let meter_id = namespace.urn("Meter", 1)?;
    let meter_type = namespace.entity_type("Meter")?;
    let meter_2_id = namespace.urn("Meter", 2)?;
    let context = Context::Single(ContextEntry::Inline(format!(r#"{{"ex": "https://test.andromeda/{namespace}/", "Meter_{namespace}": "https://test.andromeda/{namespace}/Meter", "power": "https://test.andromeda/{namespace}/power", "phase": "https://test.andromeda/{namespace}/phase"}}"#).parse::<InlineContext>()?));
    let ctx = HostedContext::add(&broker, &context).await?;
    let linked = broker.with_context(Context::Single(ctx.entry()?))?;
    let meter_evolution = TemporalEntity::builder()
        .id(meter_id.clone())
        .entity_type(meter_type.clone())
        .property("power", Property::builder(1).observed_at("2020-01-01T00:00:00Z".parse()?))
        .property("phase", Property::builder("on").observed_at("2020-01-01T00:00:00Z".parse()?))
        .build()?;
    linked.temporal().create_or_update(&meter_evolution).send().await?.created()?;
    let meter_2_evolution = TemporalEntity::builder()
        .id(meter_2_id.clone())
        .entity_type(meter_type.clone())
        .property("phase", Property::builder("off").observed_at("2020-01-01T00:00:00Z".parse()?))
        .build()?;
    linked.temporal().create_or_update(&meter_2_evolution).send().await?.created()?;
    // 5.5.7: an absolute IRI expands to itself
    let page = linked
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2000-01-01T00:00:00Z".parse()?))
        .attrs(vec![format!("https://test.andromeda/{namespace}/power").parse()?])
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(
        page.items(),
        nth(
            0,
            all!(
                has_id(&meter_id),
                has_evolution("power", nth(0, any_attribute().value(1))),
                lacks_evolution("phase")
            )
        )
    );
    // the short name, the control
    let page_2 = linked
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2000-01-01T00:00:00Z".parse()?))
        .attrs(vec!["power".parse()?])
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(1)));
    expect_that!(page_2.items(), nth(0, has_id(&meter_id)));
    // a compact IRI
    let page_3 = linked
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2000-01-01T00:00:00Z".parse()?))
        .attrs(vec!["ex:power".parse()?])
        .send()
        .await?;
    expect_that!(page_3.items(), len(eq(1)));
    expect_that!(page_3.items(), nth(0, has_id(&meter_id)));
    // an IRI among several names
    let page_4 = linked
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2000-01-01T00:00:00Z".parse()?))
        .attrs(vec![format!("https://test.andromeda/{namespace}/power").parse()?, "phase".parse()?])
        .send()
        .await?;
    expect_that!(page_4.items(), contains(has_id(&meter_id)));
    expect_that!(page_4.items(), contains(has_id(&meter_2_id)));
    // the POST binding
    let page_5 = linked
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2000-01-01T00:00:00Z".parse()?))
        .method(QueryMethod::Post)
        .attrs(vec![format!("https://test.andromeda/{namespace}/power").parse()?])
        .send()
        .await?;
    expect_that!(page_5.items(), len(eq(1)));
    expect_that!(
        page_5.items(),
        nth(
            0,
            all!(
                has_id(&meter_id),
                has_evolution("power", nth(0, any_attribute().value(1))),
                lacks_evolution("phase")
            )
        )
    );
    let body_context = broker
        .with_context(Context::Single(ctx.entry()?))?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    let page_6 = body_context
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2000-01-01T00:00:00Z".parse()?))
        .method(QueryMethod::Post)
        .attrs(vec![format!("https://test.andromeda/{namespace}/power").parse()?])
        .send()
        .await?;
    expect_that!(page_6.items(), len(eq(1)));
    expect_that!(page_6.items(), nth(0, has_id(&meter_id)));
    Ok(())
}
