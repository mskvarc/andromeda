//! The identity is answered by the broker alone beside a registration advertising the retrieve operation through its group.

use andromeda_test_suite::{
    broker,
    context_source::ContextSource,
    federation_ops_source::{expect_no_forward_reported, expect_unreached, register_federation_ops_source},
    namespace::Namespace,
    source_request::SourceRequest,
};
use googletest::prelude::*;
use ngsi_ld::{http::headers::Accept, model::NgsiLdMediaType};

/// Pins: Retrieve Context Source Identity beside a registration advertising `federationOps`, which holds
/// `retrieveContextSourceIdentity`, answers the broker's own identity and sends nothing to the source, whose
/// registration reports no forward
/// Clause: 5.15.1.4, 4.20
#[gtest]
#[tokio::test]
async fn n_inf_ret_9119_the_identity_is_answered_by_the_broker_alone_beside_a_registration_advertising_retrieve_context_source_identity_through_its_group()
-> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let registration_id = register_federation_ops_source(&broker, &source, &namespace).await?;
    let identity = broker.with_accept(Accept::new(NgsiLdMediaType::Json)).source_identity().send().await?;
    // The reader accepts only a body of type `ContextSourceIdentity`.
    expect_that!(identity.alias.to_string(), not(eq("")));
    expect_unreached(&source, &SourceRequest::any_matching("/a/.*")).await?;
    expect_no_forward_reported(&broker, &registration_id).await?;
    Ok(())
}
