//! An identifier naming no entry is resource not found.

use andromeda_test_suite::{broker, namespace::Namespace, problem::problem, wire};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{
        headers::Accept,
        operation::{ServeContext, ServeContextDetails},
    },
    model::{ContextId, ErrorType},
};

/// Pins: Serve @context, with and without details, of a segment that is no URI, a well-formed URN never stored or a
/// URL never downloaded answers 404 `ResourceNotFound` with a title
/// Clause: 5.13.4.4, 5.5.2
#[gtest]
#[tokio::test]
async fn n_ctx_srv_9003_an_identifier_naming_no_entry_is_resource_not_found() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?.with_accept(Accept::any());
    // The typed operations are built for a placeholder identifier and sent to the segment as written, since the
    // segments of this test are not identifiers the model accepts.
    let placeholder = format!("urn:ngsi-ld:Context:{namespace}-placeholder").parse::<ContextId>()?;
    let segments = [
        "wrong_id_context".to_owned(),
        "1234567890123456".to_owned(),
        "not%20a%20uri".to_owned(),
        format!("urn:ngsi-ld:Context:{namespace}-never-stored"),
        "https%3A%2F%2Fexample.invalid%2Fnever-downloaded.jsonld".to_owned(),
    ];
    for segment in &segments {
        let plain = wire::send_to(&broker, &ServeContext::builder().id(&placeholder).build(), &format!("jsonldContexts/{segment}")).await;
        expect_that!(&plain, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound).with_title());
        let details = wire::send_to(
            &broker,
            &ServeContextDetails { id: &placeholder },
            &format!("jsonldContexts/{segment}?details=true"),
        )
        .await;
        expect_that!(&details, problem(StatusCode::NOT_FOUND, ErrorType::ResourceNotFound).with_title());
    }
    Ok(())
}
