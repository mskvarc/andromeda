//! An own shaped reference with a query or another path is an ordinary url.

use andromeda_test_suite::{broker, config::SuiteConfig, hosted_context::HostedContext, namespace::Namespace, problem::problem};
use googletest::prelude::*;
use http::StatusCode;
use iri_rs::IriRefBuf;
use ngsi_ld::model::{Context, ContextEntry, Entity, ErrorType, InlineContext};

/// Pins: A create under a Link naming a stored @context's path with a query, the former tenant parameter included,
/// with an extra path segment, or under another path prefix, on an origin where nothing listens, is fetched as the
/// URL it is and answers 504 `LdContextNotAvailable`
/// Clause: 5.13.1, 6.3.5, 5.5.2
/// Recorded: clause 5.13.1 has Hosted and `ImplicitlyCreated` @contexts served from the broker's storage; Andromeda
/// resolves a reference whose path is `/ngsi-ld/v1/jsonldContexts/<identifier it holds>`, with no query, from the
/// requesting tenant's storage whatever its origin, never fetches it and never records it as Cached
#[gtest]
#[tokio::test]
async fn o_ctx_exp_0017_an_own_shaped_reference_with_a_query_or_another_path_is_an_ordinary_url() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let unreachable = SuiteConfig::get()?.unreachable_origin().origin().ascii_serialization();
    let context = Context::Single(ContextEntry::Inline(
        format!(r#"{{"ownSpeed": "https://test.andromeda/{namespace}/own/speed"}}"#).parse::<InlineContext>()?,
    ));
    let ctx = HostedContext::add(&broker, &context).await?;
    let path = ctx.url.path();
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let references = [
        format!("{unreachable}{path}?andromedaTenant={namespace}"),
        format!("{unreachable}{path}?other=1"),
        format!("{unreachable}{path}/extra"),
        format!("{unreachable}/elsewhere{path}"),
    ];
    for (n, reference) in references.into_iter().enumerate() {
        let linked = broker.with_context(Context::Single(ContextEntry::Reference(IriRefBuf::new(reference)?)))?;
        let vehicle = Entity::builder()
            .id(namespace.urn("Vehicle", n + 1)?)
            .entity_type(vehicle_type.clone())
            .property("ownSpeed", 7)
            .build()?;
        let answer = linked.with_responses().entities().create(&vehicle).send().await;
        expect_that!(&answer, problem(StatusCode::GATEWAY_TIMEOUT, ErrorType::LdContextNotAvailable));
    }
    Ok(())
}
