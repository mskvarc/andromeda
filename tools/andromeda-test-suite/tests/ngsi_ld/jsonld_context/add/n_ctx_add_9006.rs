//! A redefined type keyword is bad request data.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem};
use googletest::prelude::*;
use http::{HeaderName, StatusCode};
use ngsi_ld::{
    http::{headers::Accept, operation::ContextKindFilter},
    model::{Context, ContextEntry, ErrorType, InlineContext},
};

/// Pins: Add @context of a document redefining the keyword @type as a string is refused as `BadRequestData` without a Location, with a
/// detail naming what is wrong and where, and stores nothing
/// Clause: 5.13.2.4, 5.5.4, JSON-LD 1.1 section 9.15
/// Tenant: private (the case asserts what the tenant's whole @context store lists, and clause 4.14 keeps each
/// tenant's stored @contexts its own; in the shared tenant every other file's entries are listed too)
#[gtest]
#[tokio::test]
async fn n_ctx_add_9006_a_redefined_type_keyword_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let tenant = broker::private_tenant(&broker, &namespace).await?;
    let context = Context::Single(ContextEntry::Inline(
        format!(r#"{{"@type": "https://example.org/{namespace}/t"}}"#).parse::<InlineContext>()?,
    ));
    let answer = tenant.with_responses().contexts().add(&context).send().await;
    expect_that!(
        &answer,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData)
            .with_detail("the @context has a member @type that is a string, but an object is expected")
            .without_header(HeaderName::from_static("location"))
    );
    let contexts = tenant
        .with_accept(Accept::any())
        .contexts()
        .list()
        .kind(ContextKindFilter::Hosted)
        .send()
        .await?;
    expect_that!(contexts, len(eq(0)));
    Ok(())
}
