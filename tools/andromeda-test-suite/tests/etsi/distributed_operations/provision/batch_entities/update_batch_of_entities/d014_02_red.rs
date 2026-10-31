//! A batch update with `noOverwrite` of two entities two redirect registrations match is sent to both context sources
//! once, with the option (clauses 4.3.3, 4.3.6.3 and 5.6.9).

use crate::distributed_operations::{
    fork_rule::ForkRule,
    forwarding_source::{register, serve, source_endpoint, source_path, stub, stub_count, unregister},
    provision::batch_entities::{batch_sources::batch_registration, update_batch_of_entities::update_request::update_vehicles},
    redirected_request::{forwarded_options, wait_for_redirected},
};
use andromeda_test_suite::{broker, context_source::ContextSource, namespace::Namespace, vocabulary::Vocabulary};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{http::operation::Overwrite, registration::RegistrationMode};

/// Pins: Check that if one request the Context Broker to update a batch of entities that match a redirect
/// registration, they are updated on the Context Source
/// Clause: 4.3.3, 4.3.6.3, 5.6.9
/// Isolation: the registration's idPattern is `urn:ngsi-ld:Vehicle:<ns>-.*`: a request that names no entity type
/// matches a registration on the id alone (clause 5.12), so a pattern without the namespace would draw every other
/// test's requests on a Vehicle id into this test's source
#[gtest]
#[tokio::test]
async fn d014_02_red_update_batch_entities_with_no_overwrite_option() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Vehicle", "isParked", "providedBy", "speed", "source"]).await?;
    let source = ContextSource::refusing()?;
    let mut registrations = Vec::new();
    for (suffix, below) in [(1, "/broker1"), (2, "/broker2")] {
        let endpoint = source_endpoint(&source, &namespace, below)?;
        registrations.push(
            register(
                &broker,
                &vocabulary,
                &batch_registration(&namespace, suffix, &endpoint, Some(RegistrationMode::Redirect))?,
            )
            .await?,
        );
    }
    let _registrations = registrations;
    let source = serve(source).await?;
    let rule = |below: &str| ForkRule::resource(Method::POST, &source_path(&namespace, &format!("{below}/ngsi-ld/v1/entityoperations/update")));
    stub(&source, rule("/broker1"), StatusCode::NO_CONTENT, None).await?;
    stub(&source, rule("/broker2"), StatusCode::NO_CONTENT, None).await?;
    update_vehicles(&broker, &vocabulary, &namespace, Overwrite::Keep).await?;
    let forwarded = wait_for_redirected(&source, 0).await?;
    expect_that!(
        forwarded_options(&forwarded),
        any![elements_are![contains_substring("noOverwrite")], all![len(gt(1)), contains(eq("noOverwrite"))]]
    );
    expect_that!(stub_count(&source, &rule("/broker1")).await?, eq(1));
    expect_that!(stub_count(&source, &rule("/broker2")).await?, eq(1));
    unregister(&broker, &namespace, 1).await?;
    unregister(&broker, &namespace, 2).await
}
