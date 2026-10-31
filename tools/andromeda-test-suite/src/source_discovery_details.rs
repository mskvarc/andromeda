//! What a test's context source answers a forwarded detailed discovery request with (clauses 5.7.6 and 5.7.9, `details=true`):
//! a JSON array of the typed detail documents.

use ngsi_ld::model::{
    RawJson,
    discovery::{AttributeDetails, EntityTypeDetails},
};
use wiremock::ResponseTemplate;

/// The documents as one JSON array.
fn array(documents: impl IntoIterator<Item = RawJson>) -> String {
    let items: Vec<String> = documents.into_iter().map(|document| document.to_string()).collect();
    format!("[{}]", items.join(","))
}

/// 200 OK holding `details`, as a source answers Retrieve Details of Available Entity Types.
#[must_use]
pub fn entity_type_details(details: &[EntityTypeDetails]) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_raw(array(details.iter().map(EntityTypeDetails::render)), "application/json")
}

/// 200 OK holding `details`, as a source answers Retrieve Details of Available Attributes.
#[must_use]
pub fn attribute_details(details: &[AttributeDetails]) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_raw(array(details.iter().map(AttributeDetails::render)), "application/json")
}

#[cfg(test)]
mod tests {
    use crate::source_discovery_details::array;
    use googletest::prelude::*;
    use ngsi_ld::model::{AttributeName, Uri, discovery::AttributeDetails};

    #[gtest]
    fn documents_are_answered_as_a_json_array() -> Result<()> {
        let first = AttributeDetails::new("urn:ngsi-ld:a".parse::<Uri>()?, "speed".parse::<AttributeName>()?);
        let second = AttributeDetails::new("urn:ngsi-ld:b".parse::<Uri>()?, "fuel".parse::<AttributeName>()?);
        let text = array([first.render(), second.render()]);
        expect_that!(text, starts_with("[{"));
        expect_that!(text, contains_substring(r#""attributeName":"speed""#));
        verify_that!(text, contains_substring(r#""attributeName":"fuel""#))
    }
}
