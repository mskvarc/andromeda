//! Context values returned by a single registry persistence read.

use andromeda_json_literal::JsonLiteral;
use andromeda_ngsi_ld_model::jsonld_context::ContextMetadata;

/// A single Context projected according to the requested detail level.
#[derive(Debug)]
pub enum FetchedContext {
    /// Full lifecycle metadata.
    Detailed(Box<ContextMetadata>),
    /// The raw stored JSON-LD Context document.
    Payload(JsonLiteral),
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::fetched_context::FetchedContext;
    use andromeda_json_literal::JsonLiteral;

    #[test]
    fn payload_projection_keeps_the_context_document() {
        let fetched = FetchedContext::Payload(JsonLiteral::admit(r#"{"@context": {}}"#).expect("admissible"));
        assert!(matches!(fetched, FetchedContext::Payload(_)));
    }
}
