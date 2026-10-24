//! Holding a written document to the attribute nesting its stored expansion
//! reads back at.
//!
//! ETSI GS CIM 009 v1.9.1 bounds no nesting; the bound is the broker's, and why
//! it is what it is, and why its refusal is `BadRequestData`, is set out with
//! [`AttributeNestingLimit`].

use crate::document::{EntityDocument, instance_document::InstanceDocument};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_representation::attribute_nesting::{AttributeNestingExceeded, AttributeNestingLimit};

/// Admit a written entity document whose every attribute instance nests within
/// `limit`.
///
/// # Errors
///
/// Returns [`AttributeNestingExceeded`] naming the first attribute an instance
/// of which nests deeper.
pub fn admit_document_nesting(limit: AttributeNestingLimit, document: &EntityDocument) -> Result<(), AttributeNestingExceeded> {
    document.attributes().iter().try_for_each(|(attribute, member)| {
        member
            .instances()
            .iter()
            .try_for_each(|instance| admit_instance_nesting(limit, attribute, instance))
    })
}

/// Admit one written instance of `attribute` that nests within `limit`.
///
/// # Errors
///
/// Returns [`AttributeNestingExceeded`] naming `attribute` when the instance
/// nests deeper.
pub fn admit_instance_nesting(limit: AttributeNestingLimit, attribute: &AttributeName, instance: &InstanceDocument) -> Result<(), AttributeNestingExceeded> {
    if limit.admits(instance.nesting()) {
        Ok(())
    } else {
        Err(AttributeNestingExceeded {
            attribute: attribute.clone(), // clone: the refusal names the attribute it refuses
            limit,
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        admission::attribute_nesting_admission::{admit_document_nesting, admit_instance_nesting},
        document::{EntityDocument, instance_document::InstanceDocument},
        nesting_probe::{NESTED_ATTRIBUTE, NestingChain, nested_instance, stored_entity},
    };
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use andromeda_ngsi_ld_representation::attribute_nesting::{AttributeNestingExceeded, AttributeNestingLimit};
    use serde_json::json;
    use std::num::NonZeroUsize;

    fn limit(levels: usize) -> AttributeNestingLimit {
        AttributeNestingLimit::new(NonZeroUsize::new(levels).expect("a positive test limit"))
    }

    fn nested() -> AttributeName {
        NESTED_ATTRIBUTE.parse().expect("an attribute name")
    }

    /// An entity holding a scalar `speed` and the nested attribute in `shape`.
    fn document(shape: NestingChain, levels: usize) -> EntityDocument {
        let mut stored = stored_entity("urn:ngsi-ld:NestingProbe:1", &nested_instance(shape, levels));
        if let Some(node) = stored.get_mut(0).and_then(|node| node.as_object_mut()) {
            node.insert(
                "https://example.org/speed".to_owned(),
                json!([{"@type": ["https://uri.etsi.org/ngsi-ld/Property"], "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 1}]}]),
            );
        }
        EntityDocument::from_slice(&serde_json::to_vec(&stored).expect("JSON")).expect("a written document")
    }

    /// Every shape is admitted at the limit and refused one level past it,
    /// the refusal naming the attribute and the limit.
    #[test]
    fn a_document_is_admitted_at_the_limit_and_refused_one_level_past_it() {
        for shape in NestingChain::ALL {
            assert!(admit_document_nesting(limit(32), &document(shape, 32)).is_ok(), "{shape:?}");
            pretty_assertions::assert_eq!(
                admit_document_nesting(limit(32), &document(shape, 33)),
                Err(AttributeNestingExceeded {
                    attribute: nested(),
                    limit: limit(32),
                }),
                "{shape:?}"
            );
        }
    }

    /// Clauses 5.6.4 and 5.6.19 write one instance; it is held to the same
    /// limit.
    #[test]
    fn an_attribute_fragment_is_held_to_the_same_limit() {
        let instance = |levels| {
            InstanceDocument::from_slice(&nested(), &serde_json::to_vec(&nested_instance(NestingChain::Nodes, levels)).expect("JSON"))
                .expect("a written instance")
        };
        assert!(admit_instance_nesting(limit(32), &nested(), &instance(32)).is_ok());
        assert!(admit_instance_nesting(limit(32), &nested(), &instance(33)).is_err());
    }
}
