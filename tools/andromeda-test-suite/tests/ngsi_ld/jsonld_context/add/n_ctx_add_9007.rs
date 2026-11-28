//! A term with a number id is bad request data.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem};
use googletest::prelude::*;
use http::{HeaderName, StatusCode};
use ngsi_ld::{
    http::{headers::Accept, operation::ContextKindFilter},
    model::{Context, ContextEntry, ErrorType, InlineContext},
};

/// Pins: Add @context of a document giving the term speed an @id that is a number is refused as `BadRequestData` without a Location, with a
/// detail naming what is wrong and where, and stores nothing
/// Clause: 5.13.2.4, 5.5.4, JSON-LD 1.1 section 9.15.1
/// Tenant: private (the case asserts what the tenant's whole @context store lists, and clause 4.14 keeps each
/// tenant's stored @contexts its own; in the shared tenant every other file's entries are listed too)
#[gtest]
#[tokio::test]
async fn n_ctx_add_9007_a_term_with_a_number_id_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let tenant = broker::private_tenant(&broker, &namespace).await?;
    let context = Context::Single(ContextEntry::Inline(r#"{"speed": {"@id": 5}}"#.parse::<InlineContext>()?));
    let answer = tenant.with_responses().contexts().add(&context).send().await;
    expect_that!(
        &answer,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData)
            .with_detail(r#"the @context has a member "speed"/@id that is a number, but a string is expected"#)
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
