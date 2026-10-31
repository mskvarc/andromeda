//! A context link beside a profile link is admitted.

use andromeda_test_suite::{
    broker,
    config::SuiteConfig,
    namespace::Namespace,
    outcome::Done,
    wire::{api_uri, header_value},
};
use googletest::prelude::*;
use http::{
    HeaderValue,
    Method,
    header::{ACCEPT, LINK},
};
use iri_rs::IriRef;
use ngsi_ld::{
    http::{headers::Link, operation::RetrieveEntityTypes},
    model::Entity,
};

/// Pins: Retrieve Entity as application/json with a context Link line naming the core and a profile Link line
/// answers 200 with one @context link, to the core
/// Clause: 6.3.5, 6.3.21, 6.3.6
#[gtest]
#[tokio::test]
async fn n_ctx_exp_0032_a_context_link_beside_a_profile_link_is_admitted() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let core = SuiteConfig::get()?.core_context();
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property("speed", 1)
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    // The profile Link line is no context, and no typed read carries one beside the context Link.
    let request = broker
        .unchecked(&RetrieveEntityTypes::builder().build())?
        .method(Method::GET)
        .uri(api_uri(&broker, &format!("entities/{vehicle_id}"))?)
        .append_header(ACCEPT, HeaderValue::from_static("application/json"))
        .append_header(
            LINK,
            HeaderValue::from_str(&format!(r#"<{core}>; rel="http://www.w3.org/ns/json-ld#context"; type="application/ld+json""#))?,
        )
        .append_header(LINK, HeaderValue::from_static(r#"<https://uri.etsi.org/ngsi-ld/v1.9>; rel="profile""#));
    let response = broker.send_unchecked(request).await?;
    let Some(line) = header_value(response.headers(), &LINK) else {
        return fail!("the answer carries no single Link line");
    };
    let link: Link = line.parse()?;
    expect_that!(link.context_urls().count(), eq(1));
    expect_that!(link.context_urls().next().map(IriRef::as_str), some(eq(core.as_str())));
    Ok(())
}
