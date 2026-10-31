//! An empty registration info is bad request data.

use andromeda_test_suite::{broker, namespace::Namespace, refused_body::expect_bad_request_data};
use googletest::prelude::*;
use http::Method;

/// Pins: Create Context Source Registration whose information holds an empty `RegistrationInfo`, an empty entities
/// array, an empty `propertyNames` array or an empty `relationshipNames` array answers 400 `BadRequestData`
/// Clause: 5.9.2.4, 5.2.9, 5.2.10
#[gtest]
#[tokio::test]
async fn n_csr_cre_6008_an_empty_registration_info_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    // Each registration holds an empty member the model refuses to write, so the bodies are written as text.
    let bodies = [
        ("1", r"[{}]"),
        ("2", r#"[{"entities":[]}]"#),
        ("3", r#"[{"propertyNames":[]}]"#),
        ("4", r#"[{"relationshipNames":[]}]"#),
    ]
    .map(|(suffix, information)| {
        format!(r#"{{"id":"urn:ngsi-ld:ContextSourceRegistration:{namespace}-{suffix}","type":"ContextSourceRegistration","information":{information},"endpoint":"http://source.test/{suffix}"}}"#)
    });
    expect_bad_request_data(&broker, &Method::POST, "csourceRegistrations", &bodies).await?;
    Ok(())
}
