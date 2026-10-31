//! An identifier naming no entry is resource not found.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem, wire::api_uri};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::operation::{ContextDeletion, DeleteContext},
    model::{ContextId, ErrorType},
};

/// Pins: Delete @context of a segment that is no URI, a well-formed URN never stored or a URL never downloaded answers
/// 404 `ResourceNotFound` with a title, without reload and with reload true or false
/// Clause: 5.13.5.4, 5.5.2
#[gtest]
#[tokio::test]
async fn n_ctx_del_9003_an_identifier_naming_no_entry_is_resource_not_found() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let placeholder_id = namespace.urn_text("JsonLdContext", "placeholder").parse::<ContextId>()?;
    let never_stored = format!("urn:ngsi-ld:Context:{namespace}-never-stored");
    let segments = [
        "wrong_id_context",
        "1234567890123456",
        "not%20a%20uri",
        never_stored.as_str(),
        "https%3A%2F%2Fexample.invalid%2Fnever-downloaded.jsonld",
    ];
    let variants = [
        (ContextDeletion::Remove, ""),
        (ContextDeletion::Reload, "?reload=true"),
        (ContextDeletion::RemoveExplicitly, "?reload=false"),
    ];
    for segment in segments {
        for (deletion, query) in variants {
            // The model refuses a segment that is no URI and encodes one that is, so the operation is built for a
            // placeholder and sent to the path as written.
            let request = broker
                .unchecked(&DeleteContext::builder().id(&placeholder_id).deletion(deletion).build())?
                .uri(api_uri(&broker, &format!("jsonldContexts/{segment}{query}"))?);
            let answer = broker.send_unchecked(request).await;
            expect_that!(&answer, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound).with_title());
        }
    }
    Ok(())
}
