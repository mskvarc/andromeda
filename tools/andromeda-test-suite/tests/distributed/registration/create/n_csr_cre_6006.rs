//! A member breaking its definition is bad request data.

use andromeda_test_suite::{broker, namespace::Namespace, refused_body::expect_bad_request_data};
use googletest::prelude::*;
use http::Method;

/// Pins: Create Context Source Registration with an id that is no URI, a missing or wrong type, a missing or
/// invalid endpoint, a missing or empty information, an empty `contextSourceAlias`, description or
/// `registrationName`, a `datasetId` that is no URI, an invalid `expiresAt`, an unknown mode, an invalid
/// `refreshRate`, or two such members at once answers 400 `BadRequestData`
/// Clause: 5.9.2.4, 5.2.9
#[gtest]
#[tokio::test]
async fn n_csr_cre_6006_a_member_breaking_its_definition_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let id = |suffix: u32| format!("urn:ngsi-ld:ContextSourceRegistration:{namespace}-{suffix}");
    let information = format!(r#"[{{"entities":[{{"idPattern":"urn:ngsi-ld:Vehicle:{namespace}-.*","type":"Vehicle_{namespace}"}}]}}]"#);
    let registration = |suffix: u32, extra: &str| {
        format!(
            r#"{{"id":"{}","type":"ContextSourceRegistration","information":{information},"endpoint":"http://source.test/{suffix}"{extra}}}"#,
            id(suffix)
        )
    };
    // Each registration breaks a definition the model enforces, so the bodies are written as text.
    let bodies = [
        format!(r#"{{"id":"no-scheme-{namespace}","type":"ContextSourceRegistration","information":{information},"endpoint":"http://source.test/1"}}"#),
        format!(r#"{{"id":"{}","information":{information},"endpoint":"http://source.test/2"}}"#, id(2)),
        format!(
            r#"{{"id":"{}","type":"Foo_{namespace}","information":{information},"endpoint":"http://source.test/3"}}"#,
            id(3)
        ),
        format!(r#"{{"id":"{}","type":"ContextSourceRegistration","information":{information}}}"#, id(4)),
        format!(
            "{{\"id\":\"{}\",\"type\":\"ContextSourceRegistration\",\"information\":{information},\"endpoint\":\"###not a uri###\"}}",
            id(5)
        ),
        format!(r#"{{"id":"{}","type":"ContextSourceRegistration","endpoint":"http://source.test/6"}}"#, id(6)),
        format!(
            r#"{{"id":"{}","type":"ContextSourceRegistration","information":[],"endpoint":"http://source.test/7"}}"#,
            id(7)
        ),
        registration(8, r#","contextSourceAlias":"""#),
        registration(9, r#","description":"""#),
        registration(10, r#","registrationName":"""#),
        registration(11, r#","datasetId":["not a uri"]"#),
        registration(12, r#","expiresAt":"not-a-date""#),
        registration(13, r#","mode":"foo""#),
        registration(14, r#","refreshRate":"5 minutes""#),
        format!(
            r#"{{"id":"{}","type":"ContextSourceRegistration","information":{information},"mode":"foo"}}"#,
            id(15)
        ),
    ];
    expect_bad_request_data(&broker, &Method::POST, "csourceRegistrations", &bodies).await?;
    Ok(())
}
