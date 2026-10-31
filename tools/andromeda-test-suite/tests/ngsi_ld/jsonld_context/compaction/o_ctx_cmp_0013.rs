//! The source identity names its context through the default tenant.

use andromeda_test_suite::{
    broker,
    core_context::core_entry,
    minted_wrapper::{minted_wrapper, served_document},
    namespace::Namespace,
    remote_context::RemoteContexts,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Context, NgsiLdMediaType},
};

/// Pins: Retrieve Source Identity as json under a user @context links one wrapper of the default tenant, naming no
/// tenant, that dereferences to the user @context followed by the core; as ld+json it carries that pair in the body,
/// and with no Link the core alone
/// Clause: 6.3.6, 5.15, 5.13.1
/// Recorded: clause 6.3.6 has an application/json answer link its @context but does not say how a @context that no
/// single URL names is linked; Andromeda hosts the value an application/ld+json body would carry as an
/// `ImplicitlyCreated` @context (clause 5.13.1) and links its absolute URL
#[gtest]
#[tokio::test]
async fn o_ctx_cmp_0013_the_source_identity_names_its_context_through_the_default_tenant() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let contexts = RemoteContexts::start().await?;
    let linked = broker.with_context(Context::Single(contexts.reference("jc/user/terms.jsonld")?))?;
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
    let headed = linked
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .source_identity()
        .send()
        .await?;
    let wrapper = minted_wrapper(&headed.head)?;
    let document = served_document(&broker, &wrapper).await?;
    expect_that!(document.extra_members, is_empty());
    expect_that!(
        document.context,
        eq(&Context::Multiple(vec![contexts.reference("jc/user/terms.jsonld")?, core_entry()?]))
    );
    let identity = linked.with_accept(Accept::new(NgsiLdMediaType::JsonLd)).source_identity().send().await?;
    expect_that!(
        identity.context.as_ref(),
        some(eq(&Context::Multiple(vec![contexts.reference("jc/user/terms.jsonld")?, core_entry()?])))
    );
    let identity_2 = broker.with_accept(Accept::new(NgsiLdMediaType::JsonLd)).source_identity().send().await?;
    expect_that!(identity_2.context.as_ref(), some(eq(&Context::Single(core_entry()?))));
    Ok(())
}
