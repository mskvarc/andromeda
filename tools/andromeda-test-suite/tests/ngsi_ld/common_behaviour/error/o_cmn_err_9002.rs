//! An unknown tenant refusal names the request target.

use andromeda_test_suite::{
    broker,
    namespace::Namespace,
    problem::{instance_of, problem},
    wire::request_to,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::model::{ErrorType, Tenant};

/// Pins: Retrieve Available Entity Types, Retrieve Available Attributes with details=true and Query Entities
/// against a tenant that does not exist are each refused with `NonexistentTenant` and an instance that is the
/// request target
/// Clause: 5.5.3, 5.5.10, 6.3.2
/// Tenant: refused (the subject is the refusal of a tenant that does not exist; none is created)
/// Recorded: 5.5.3 and IETF RFC 7807 section 3.1 make instance "a URI reference that identifies the specific
/// occurrence of the problem" without saying which; Andromeda names the request target the client sent, path and
/// query string
#[gtest]
#[tokio::test]
async fn o_cmn_err_9002_an_unknown_tenant_refusal_names_the_request_target() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let absent = broker.with_tenant(format!("absent{namespace}").parse::<Tenant>()?);
    let targets = [
        "types".to_owned(),
        "attributes?details=true".to_owned(),
        format!("entities?type=Building_{namespace}"),
    ];
    for target in &targets {
        // The request target is sent as written, since the instance of the problem is compared with it.
        let answer = absent.send_unchecked(request_to(&absent, Method::GET, target)?).await;
        expect_that!(&answer, problem(StatusCode::NOT_FOUND, ErrorType::NonexistentTenant));
        let instance = format!("/ngsi-ld/v1/{target}");
        expect_that!(instance_of(&answer), some(eq(instance.as_str())));
    }
    Ok(())
}
