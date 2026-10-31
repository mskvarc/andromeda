//! A member spelled as a short name and as its iri in pick and omit is refused at every entry point.

use andromeda_test_suite::{
    attribute_matcher::property,
    broker,
    config::SuiteConfig,
    entity_matcher::{has_attribute, lacks_attribute},
    hosted_context::HostedContext,
    namespace::Namespace,
    outcome::Done,
    problem::problem,
    raw_call::target,
    unchecked_call::exchange,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{Context, ContextEntry, Entity, ErrorType, InlineContext, NgsiLdMediaType, Property},
    query::projection::Projection,
};

/// Pins: under a user @context mapping name to an IRI, pick and omit naming that member once as name and once as
/// the IRI answer 400 `BadRequestData` on Retrieve Entity, Query Entities, a POST query, both temporal reads,
/// Create Subscription and Create Context Source Registration Subscription, while two short names mapped to
/// different IRIs stay admitted
/// Clause: 4.21, 5.5.7, 5.7.1.4, 5.7.2.4, 5.7.3.4, 5.7.4.4, 5.8.1.4, 5.11.2.3
/// Recorded: clause 5.5.7 makes every pick and omit name a term of the request @context, so the two spellings are
/// one member; a member named in both is refused as a self-contradictory projection with `BadRequestData`, ruled by
/// the user (2026-10-11)
#[gtest]
#[tokio::test]
async fn o_qry_prj_9010_a_member_spelled_as_a_short_name_and_as_its_iri_in_pick_and_omit_is_refused_at_every_entry_point() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let config = SuiteConfig::get()?;
    let vehicle_id = namespace.urn("Vehicle", "spellings")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let context = Context::Single(ContextEntry::Inline(
        format!(
            r#"{{"Vehicle_{namespace}": "https://test.andromeda/{namespace}/Vehicle", "name": "https://test.andromeda/{namespace}/name", "label": "https://test.andromeda/{namespace}/label"}}"#
        )
        .parse::<InlineContext>()?,
    ));
    let hosted = HostedContext::add(&broker, &context).await?;
    let linked = broker.with_context(Context::Single(hosted.entry()?))?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("name", Property::builder("n1").observed_at("2025-01-01T00:00:00Z".parse()?))
        .property("label", Property::builder("l1").observed_at("2025-01-01T00:00:00Z".parse()?))
        .build()?;
    linked.entities().create(&vehicle).send().await?.done()?;
    let name_iri = format!("https://test.andromeda/{namespace}/name");
    let link = format!(r#"<{}>; rel="http://www.w3.org/ns/json-ld#context"; type="application/ld+json""#, hosted.url);
    let refusal = || problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData);
    // A member spelled as its IRI is a projection term the model refuses, so every refused call goes out as written.
    let get = [("Link", link.as_str())];
    let type_text = vehicle_type.to_string();
    let time = [("timerel", "after"), ("timeAt", "2000-01-01T00:00:00Z")];
    let spellings = [("name", name_iri.as_str()), (name_iri.as_str(), "name")];
    for (pick, omit) in spellings {
        let projection = [("pick", pick), ("omit", omit)];
        let mut by_type = vec![("type", type_text.as_str())];
        by_type.extend(projection);
        let mut temporal = time.to_vec();
        temporal.extend(projection);
        let mut temporal_by_type = vec![("type", type_text.as_str())];
        temporal_by_type.extend(time);
        temporal_by_type.extend(projection);
        let reads = [
            target(&format!("entities/{vehicle_id}"), &projection),
            target("entities", &by_type),
            target(&format!("temporal/entities/{vehicle_id}"), &temporal),
            target("temporal/entities", &temporal_by_type),
        ];
        for read in &reads {
            let answer = broker.send_unchecked(exchange(&broker, Method::GET, read, &get, None)?).await;
            expect_that!(&answer, refusal());
        }
        let body = format!(r#"{{"type": "Query", "entities": [{{"type": "{vehicle_type}"}}], "pick": ["{pick}"], "omit": ["{omit}"]}}"#);
        let posted = broker
            .send_unchecked(exchange(
                &broker,
                Method::POST,
                "entityOperations/query",
                &[("Content-Type", "application/json"), ("Link", link.as_str())],
                Some(body.as_str()),
            )?)
            .await;
        expect_that!(&posted, refusal());
    }
    let json = linked.with_accept(Accept::new(NgsiLdMediaType::Json));
    let projected = json
        .entities()
        .retrieve_projected(&vehicle_id)
        .pick("name".parse::<Projection>()?)
        .omit("label".parse::<Projection>()?)
        .send()
        .await?;
    expect_that!(projected.body(), has_attribute("name", property().value("n1")));
    expect_that!(projected.body(), lacks_attribute("label"));
    expect_that!(projected.body().id, none());
    let subscription_id = namespace.subscription_id("spellings")?;
    let subscription = format!(
        r#"{{"id": "{subscription_id}", "type": "Subscription", "entities": [{{"type": "{vehicle_type}"}}], "notification": {{"endpoint": {{"uri": "{}notify", "accept": "application/json"}}, "pick": ["name"], "omit": ["{name_iri}"]}}}}"#,
        config.unreachable_origin()
    );
    let created = broker
        .send_unchecked(exchange(
            &broker,
            Method::POST,
            "subscriptions",
            &[("Content-Type", "application/json"), ("Link", link.as_str())],
            Some(subscription.as_str()),
        )?)
        .await;
    expect_that!(&created, refusal());
    let missing = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .subscriptions()
        .retrieve(&subscription_id)
        .send()
        .await;
    expect_that!(&missing, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound));
    let registration_subscription = subscription.replace(&subscription_id.to_string(), &namespace.subscription_id("spellings-csource")?.to_string());
    let csource = broker
        .send_unchecked(exchange(
            &broker,
            Method::POST,
            "csourceSubscriptions",
            &[("Content-Type", "application/json"), ("Link", link.as_str())],
            Some(registration_subscription.as_str()),
        )?)
        .await;
    expect_that!(&csource, refusal());
    Ok(())
}
