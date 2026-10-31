//! Add a single url stores a context.

use andromeda_test_suite::{broker, hosted_context::HostedContext, namespace::Namespace, remote_context::RemoteContexts};
use googletest::prelude::*;
use ngsi_ld::model::Context;

/// Pins: Add @context whose @context member is a single URL string answers 201 with a Location
/// Clause: 5.13.2.3, 5.13.2.4, 6.29.3.1
#[gtest]
#[tokio::test]
async fn n_ctx_add_0010_add_a_single_url_stores_a_context() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let contexts = RemoteContexts::start().await?;
    contexts
        .serve(
            "jc/single.jsonld",
            &format!(
                r#"{{
      "@context": {{
        "A1": "https://test.andromeda/{namespace}/A1"
      }}
    }}"#
            ),
            Some("application/ld+json"),
        )
        .await?;
    let context = Context::Single(contexts.reference("jc/single.jsonld")?);
    HostedContext::add(&broker, &context).await?;
    Ok(())
}
