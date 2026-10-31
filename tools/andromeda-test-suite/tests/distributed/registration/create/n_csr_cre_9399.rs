//! A tenant key in context source info is stored and the registration created.

use andromeda_test_suite::{broker, cleanup::Cleanup, context_source::ContextSource, namespace::Namespace, problem::answered, written_json::json_request_to};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::http::operation::RetrieveRegistration;
use serde_json::{Value, json};

/// Pins: Create Context Source Registration whose `contextSourceInfo` holds an `NGSILD-Tenant` entry, in either
/// casing, answers 201 and returns the entry as written
/// Clause: 4.3.6.5, 5.9.2.4
#[gtest]
#[tokio::test]
async fn n_csr_cre_9399_a_tenant_key_in_context_source_info_is_stored_and_the_registration_created() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    // The model refuses the tenant key (a registration contextSourceInfo), so the documents are written as text.
    for (suffix, key) in [(1, "NGSILD-Tenant"), (2, "ngsild-tenant")] {
        let id = namespace.registration_id(suffix)?;
        let body = format!(
            r#"{{"id": "{id}", "type": "ContextSourceRegistration", "information": [{{"entities": [{{"type": "{vehicle_type}", "idPattern": "urn:ngsi-ld:Vehicle:{namespace}-.*"}}]}}], "endpoint": "{endpoint}", "mode": "inclusive", "contextSourceInfo": [{{"key": "{key}", "value": "t1"}}, {{"key": "Content-Length", "value": "10"}}]}}"#,
            endpoint = source.endpoint("1")?
        );
        let answer = broker
            .send_unchecked(json_request_to(&broker, Method::POST, "csourceRegistrations", &body)?)
            .await;
        expect_that!(&answer, answered(StatusCode::CREATED));
        let _registration_cleanup = Cleanup::of(&broker).registration(&id);
        let read = broker.send_unchecked(broker.unchecked(&RetrieveRegistration::builder().id(&id).build())?).await;
        verify_that!(&read, answered(StatusCode::OK))?;
        let stored = serde_json::from_slice::<Value>(read?.body())?;
        expect_that!(
            stored.pointer("/contextSourceInfo"),
            some(eq(&json!([{"key": key, "value": "t1"}, {"key": "Content-Length", "value": "10"}])))
        );
    }
    Ok(())
}
