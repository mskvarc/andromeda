//! What a test's context source answers a forwarded discovery request with (clauses 5.7.5 to 5.7.12), written from
//! the typed discovery documents.

use ngsi_ld::model::discovery::{AttributeInfo, AttributeList, EntityTypeInfo, EntityTypeList};
use wiremock::ResponseTemplate;

/// 200 OK holding the JSON document `body`.
fn document(body: String) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_raw(body, "application/json")
}

/// 200 OK holding `list`, as a source answers Retrieve Available Entity Types.
#[must_use]
pub fn entity_type_list(list: &EntityTypeList) -> ResponseTemplate {
    document(list.render().to_string())
}

/// 200 OK holding `list`, as a source answers Retrieve Available Attributes.
#[must_use]
pub fn attribute_list(list: &AttributeList) -> ResponseTemplate {
    document(list.render().to_string())
}

/// 200 OK holding `information`, as a source answers Retrieve Available Entity Type Information.
#[must_use]
pub fn entity_type_information(information: &EntityTypeInfo) -> ResponseTemplate {
    document(information.render().to_string())
}

/// 200 OK holding `information`, as a source answers Retrieve Available Attribute Information.
#[must_use]
pub fn attribute_information(information: &AttributeInfo) -> ResponseTemplate {
    document(information.render().to_string())
}

#[cfg(test)]
mod tests {
    use crate::source_discovery::{attribute_list, entity_type_list};
    use googletest::prelude::*;
    use ngsi_ld::model::{
        AttributeName,
        EntityTypeName,
        Uri,
        discovery::{AttributeList, EntityTypeList},
    };
    use wiremock::{Mock, MockServer, matchers::method};

    #[gtest]
    #[tokio::test(flavor = "multi_thread")]
    async fn an_entity_type_list_is_answered_as_a_json_document() -> Result<()> {
        let list = EntityTypeList::new("urn:ngsi-ld:EntityTypeList:1".parse::<Uri>()?, vec!["Car".parse::<EntityTypeName>()?]);
        let server = MockServer::start().await;
        Mock::given(method("GET")).respond_with(entity_type_list(&list)).mount(&server).await;
        let answer = reqwest::get(server.uri()).await?;
        expect_that!(answer.status().as_u16(), eq(200));
        verify_that!(EntityTypeList::parse(&answer.text().await?)?, eq(&list))
    }

    #[gtest]
    #[tokio::test(flavor = "multi_thread")]
    async fn an_attribute_list_is_answered_as_a_json_document() -> Result<()> {
        let list = AttributeList::new("urn:ngsi-ld:AttributeList:1".parse::<Uri>()?, vec!["speed".parse::<AttributeName>()?]);
        let server = MockServer::start().await;
        Mock::given(method("GET")).respond_with(attribute_list(&list)).mount(&server).await;
        let answer = reqwest::get(server.uri()).await?;
        expect_that!(answer.status().as_u16(), eq(200));
        verify_that!(AttributeList::parse(&answer.text().await?)?, eq(&list))
    }
}
