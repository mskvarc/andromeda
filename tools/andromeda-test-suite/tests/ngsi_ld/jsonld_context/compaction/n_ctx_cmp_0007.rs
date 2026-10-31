//! A batch create answer names at most one context link.

use andromeda_test_suite::{broker, minted_wrapper::linked_context, namespace::Namespace, outcome::Done, remote_context::RemoteContexts};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::model::{Context, Entity};

/// Pins: A json Batch Entity Creation under a user @context answered 201 with its body carries exactly one @context
/// link
/// Clause: 6.3.6, JSON-LD 1.1 s6.1
#[gtest]
#[tokio::test]
async fn n_ctx_cmp_0007_a_batch_create_answer_names_at_most_one_context_link() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let contexts = RemoteContexts::start().await?;
    let linked = broker.with_context(Context::Single(contexts.reference("jc/user/terms.jsonld")?))?;
    let device_type = namespace.entity_type("Device")?;
    contexts
        .serve(
            "jc/user/terms.jsonld",
            &format!(
                r#"{{
      "@context": {{
        "Device_{namespace}": "https://test.andromeda/{namespace}/Device",
        "speed": "https://test.andromeda/{namespace}/speed"
      }}
    }}"#
            ),
            Some("application/ld+json"),
        )
        .await?;
    let devices = [
        Entity::builder().id(namespace.urn("Device", 1)?).entity_type(device_type.clone()).build()?,
        Entity::builder().id(namespace.urn("Device", 2)?).entity_type(device_type).build()?,
    ];
    let headed = linked.with_heads().entities().batch_create(&devices).send().await?;
    expect_that!(headed.status, eq(StatusCode::CREATED));
    headed.outcome.done()?;
    expect_that!(linked_context(&headed.head), ok(anything()));
    Ok(())
}
