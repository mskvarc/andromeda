//! What a test's context source answers under `application/ld+json` (clause 6.3.5): entities that name their own
//! `@context` in the body rather than in a `Link` header.

use ngsi_ld::model::{Entity, Output, Representation};
use wiremock::ResponseTemplate;

/// 200 OK holding `entities` in normalized representation as a JSON-LD document, each entity writing its own
/// `@context` when it holds one, as a source answers Query Entities to a broker it let read its context from the body.
#[must_use]
pub fn entities(entities: &[Entity<Output>]) -> ResponseTemplate {
    let items: Vec<String> = entities.iter().map(|entity| entity.render(Representation::Normalized).to_string()).collect();
    ResponseTemplate::new(200).set_body_raw(format!("[{}]", items.join(",")), "application/ld+json")
}

#[cfg(test)]
mod tests {
    use crate::source_ld_json::entities;
    use googletest::prelude::*;
    use iri_rs::IriBuf;
    use ngsi_ld::model::Entity;
    use wiremock::{Mock, MockServer, matchers::method};

    #[gtest]
    #[tokio::test(flavor = "multi_thread")]
    async fn entities_are_answered_as_a_json_ld_array_with_their_context() -> Result<()> {
        let device = Entity::output_builder()
            .id("urn:ngsi-ld:Device:1")
            .entity_type("Device")
            .context(IriBuf::new("https://example.org/context.jsonld".to_owned()).ok().or_fail()?)
            .build()?;
        let server = MockServer::start().await;
        Mock::given(method("GET")).respond_with(entities(&[device])).mount(&server).await;
        let answer = reqwest::get(server.uri()).await?;
        expect_that!(
            answer.headers().get("content-type").and_then(|value| value.to_str().ok()),
            some(eq("application/ld+json"))
        );
        verify_that!(answer.text().await?, contains_substring(r#""@context":"https://example.org/context.jsonld""#))
    }
}
