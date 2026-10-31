//! An entity selector outside the entity info definition is bad request data.

use andromeda_test_suite::{broker, namespace::Namespace, refused_body::expect_bad_request_data};
use googletest::prelude::*;
use http::Method;

/// Pins: Create Context Source Registration whose `EntityInfo` has no type (id only, `idPattern` only), an id that
/// is no URI or an `idPattern` that is no regular expression answers 400 `BadRequestData`
/// Clause: 5.9.2.4, 5.2.8
#[gtest]
#[tokio::test]
async fn n_csr_cre_6005_an_entity_selector_outside_the_entity_info_definition_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    // Each registration breaks the definition of its `EntityInfo`, which the model enforces, so the bodies are written as text.
    let bodies = [
        format!(
            r#"{{"id":"urn:ngsi-ld:ContextSourceRegistration:{namespace}-1","type":"ContextSourceRegistration","information":[{{"entities":[{{"id":"urn:ngsi-ld:Vehicle:{namespace}-1"}}]}}],"endpoint":"http://source.test/1"}}"#
        ),
        format!(
            r#"{{"id":"urn:ngsi-ld:ContextSourceRegistration:{namespace}-2","type":"ContextSourceRegistration","information":[{{"entities":[{{"idPattern":"urn:ngsi-ld:Vehicle:{namespace}-.*"}}]}}],"endpoint":"http://source.test/2"}}"#
        ),
        format!(
            r#"{{"id":"urn:ngsi-ld:ContextSourceRegistration:{namespace}-3","type":"ContextSourceRegistration","information":[{{"entities":[{{"type":"Vehicle_{namespace}","id":"no-scheme-{namespace}"}}]}}],"endpoint":"http://source.test/3"}}"#
        ),
        format!(
            r#"{{"id":"urn:ngsi-ld:ContextSourceRegistration:{namespace}-4","type":"ContextSourceRegistration","information":[{{"entities":[{{"type":"Vehicle_{namespace}","idPattern":"[unterminated"}}]}}],"endpoint":"http://source.test/4"}}"#
        ),
    ];
    expect_bad_request_data(&broker, &Method::POST, "csourceRegistrations", &bodies).await?;
    Ok(())
}
