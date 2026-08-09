//! Context values returned by a single registry persistence read.

use andromeda_ngsi_ld_model::jsonld_context::ContextMetadata;
use serde_json::Value;

/// A single Context projected according to the requested detail level.
#[derive(Debug)]
pub enum FetchedContext {
    /// Full lifecycle metadata.
    Detailed(Box<ContextMetadata>),
    /// The raw stored JSON-LD Context document.
    Payload(Value),
}

#[cfg(test)]
mod tests {
    use crate::fetched_context::FetchedContext;

    #[test]
    fn payload_projection_keeps_the_context_document() {
        let fetched = FetchedContext::Payload(serde_json::json!({ "@context": {} }));
        assert!(matches!(fetched, FetchedContext::Payload(_)));
    }
}
