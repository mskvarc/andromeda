//! Clause C.3: the Context Source Registration example.
//!
//! The registration names an endpoint that does not exist. It is deleted
//! before anything is asserted, because a live registration pointing at an
//! unreachable Context Source turns every later write in the tenant into a
//! partially-successful multi-status answer.

use crate::{
    BASE_URL,
    clean_tenant,
    client,
    comparison::{ContextMember, EntityOrder, assert_matches_example, corpus},
    context_link,
    example_context::hosted_contexts,
    get_json,
};
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::Value;
use serial_test::serial;

/// The registration exactly as clause C.3 prints it.
const REGISTRATION: &str = include_str!("corpus/c-3-registration.json");
/// The identifier the printed registration claims.
const REGISTRATION_ID: &str = "urn:ngsi-ld:ContextSourceRegistration:csr1a3456";

#[tokio::test]
#[serial]
async fn annex_c_3_context_source_registration_example() {
    let environment = clean_tenant().await;
    let link = context_link(hosted_contexts().url("aggregatedContext.jsonld").as_str());
    let expected = corpus(REGISTRATION);
    let http = client();

    let response = http
        .post(format!("{BASE_URL}/csourceRegistrations/"))
        .header("Content-Type", "application/json")
        .header("Link", &link)
        .json(&crate::without_context(&expected))
        .send()
        .await
        .expect("registration creation request");
    let created = response.status();
    let refusal = response.text().await.unwrap_or_default();

    let retrieved = if created == StatusCode::CREATED {
        let (status, body) = get_json(&http, &format!("/csourceRegistrations/{REGISTRATION_ID}"), "application/ld+json", Some(&link)).await;
        Some((status, body))
    } else {
        None
    };

    // Remove the registration before asserting anything, so a mismatch cannot
    // leave a Context Source pointing at a dead endpoint behind it.
    let _deleted = http.delete(format!("{BASE_URL}/csourceRegistrations/{REGISTRATION_ID}")).send().await;

    assert_eq!(created, StatusCode::CREATED, "the C.3 registration example was refused: {refusal}");
    let (status, actual) = retrieved.expect("a created registration is retrievable");
    assert_eq!(status, StatusCode::OK, "registration retrieval failed: {actual}");
    assert_matches_example("C.3", &expected, &without_defaults(actual), ContextMember::Ignored, EntityOrder::AsPrinted);

    environment.reset_and_finish().await.expect("tenant reset");
}

/// Drop the members clause 5.2.9 marks output-only or gives a default.
///
/// `status` and the delivery counters report how the broker has been getting on
/// with the Context Source since registration. `mode` and `operations` are the
/// defaults clause 5.2.9 defines (`inclusive` and `federationOps`), which the
/// printed registration leaves implicit. Neither group is part of what the
/// example demonstrates.
///
/// The `endpoint` is also brought back to the form the example prints: the
/// broker stores it with the explicit empty path IETF RFC 3986 clause 6.2.3
/// makes equivalent to none, so `http://my.csource.org:1026` reads back as
/// `http://my.csource.org:1026/`.
fn without_defaults(mut registration: Value) -> Value {
    if let Some(members) = registration.as_object_mut() {
        for member in [
            "status",
            "timesSent",
            "timesFailed",
            "lastSuccess",
            "lastFailure",
            "lastSuccessfulOperation",
            "mode",
            "operations",
        ] {
            members.remove(member);
        }
        if let Some(Value::String(endpoint)) = members.get_mut("endpoint") {
            let trimmed = endpoint.trim_end_matches('/').to_owned();
            *endpoint = trimmed;
        }
    }
    registration
}
