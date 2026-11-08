//! Compacting a stored registration for the response that carries it.
//!
//! A registration is stored expanded, so retrieval and discovery compact the
//! names it holds (the entity types and attribute names of `information`, and
//! the names of its Context Source Properties) through the `@context` the
//! response is rendered under (clauses 5.10.2.5 and 5.10.4.5).

use crate::{context_source_properties::compact_context_source_properties, information_compaction::compacted_information};
use andromeda_ngsi_ld_compaction::name_compaction::NameCompaction;
use andromeda_ngsi_ld_model::registration::ContextSourceRegistration;
use std::mem::take;

/// `registration` with every name it holds compacted with `names`.
pub(crate) fn compacted_registration(names: &NameCompaction, mut registration: ContextSourceRegistration) -> ContextSourceRegistration {
    registration.coverage.information = compacted_information(names, &registration.coverage.information);
    registration.additional_properties = compact_context_source_properties(names, take(&mut registration.additional_properties));
    registration
}

#[cfg(test)]
mod tests {
    use crate::registration_compaction::compacted_registration;
    use andromeda_jsonld::{context_spec::ContextSpec, engine::processor::JsonLdEngine};
    use andromeda_ngsi_ld_compaction::name_compaction::NameCompaction;
    use andromeda_ngsi_ld_model::registration::ContextSourceRegistration;
    use serde_json::json;

    /// Clause 5.10.4.5: the names a registration holds compact under the
    /// response's context, whichever member holds them.
    #[tokio::test]
    async fn the_information_and_the_properties_of_a_registration_compact_alike() {
        let engine = JsonLdEngine::builder().allow_remote(false).build().expect("engine builds");
        let names = NameCompaction::new(
            engine
                .compactor(&ContextSpec::none())
                .processed_context()
                .await
                .expect("the core context processes"),
        );
        let stored: ContextSourceRegistration = serde_json::from_value(json!({
            "id": "urn:ngsi-ld:ContextSourceRegistration:compact",
            "type": "ContextSourceRegistration",
            "information": [{"entities": [{"type": "https://uri.etsi.org/ngsi-ld/default-context/Vehicle"}]}],
            "endpoint": "https://source.example.com/",
            "createdAt": "2024-01-01T00:00:00Z",
            "modifiedAt": "2024-01-02T00:00:00Z",
            "https://uri.etsi.org/ngsi-ld/default-context/tier": 3
        }))
        .expect("a stored registration");

        let written = serde_json::to_value(compacted_registration(&names, stored)).expect("a registration writes");

        pretty_assertions::assert_eq!(written["information"], json!([{"entities": [{"type": "Vehicle"}]}]));
        pretty_assertions::assert_eq!(written["tier"], json!(3));
        assert!(written.get("https://uri.etsi.org/ngsi-ld/default-context/tier").is_none(), "{written}");
    }
}
