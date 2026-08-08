//! Typed IRIs generated from the embedded NGSI-LD vocabulary.

use crate::error::JsonLdError;
use andromeda_ngsi_ld_vocabulary::COMPACT_TO_EXPANDED;
use iri_rs::IriBuf;
use std::collections::HashMap;

/// The compile-time vocabulary after every generated IRI has been validated.
#[derive(Debug)]
pub(crate) struct CoreVocabulary {
    /// Validated compact-term to expanded-IRI mappings.
    terms: HashMap<Box<str>, IriBuf>,
}

impl CoreVocabulary {
    /// Validate and store the generated vocabulary before an engine is exposed.
    pub(crate) fn try_from_generated() -> Result<CoreVocabulary, JsonLdError> {
        Self::try_from_entries(COMPACT_TO_EXPANDED.iter().copied())
    }

    /// Iterate over the validated compact-term mappings.
    pub(crate) fn iter(&self) -> impl Iterator<Item = (&str, &IriBuf)> {
        self.terms.iter().map(|(compact, expanded)| (compact.as_ref(), expanded))
    }

    /// Validate generated vocabulary entries before storing them.
    fn try_from_entries<'a>(entries: impl IntoIterator<Item = (&'a str, &'a str)>) -> Result<CoreVocabulary, JsonLdError> {
        let terms = entries
            .into_iter()
            .map(|(compact, expanded)| {
                IriBuf::new(expanded.to_owned())
                    .map(|iri| (Box::<str>::from(compact), iri))
                    .map_err(|error| JsonLdError::invalid_iri(format!("generated vocabulary term {compact}: {}", error.0)))
            })
            .collect::<Result<HashMap<_, _>, _>>()?;
        Ok(CoreVocabulary { terms })
    }
}

#[cfg(test)]
mod tests {
    use crate::{engine::term_map::vocabulary::CoreVocabulary, error::JsonLdError};

    #[test]
    fn malformed_generated_vocabulary_returns_a_typed_error() {
        let result = CoreVocabulary::try_from_entries([("invalid", "not an IRI")]);

        assert!(matches!(result, Err(JsonLdError::InvalidIri(_))));
    }

    #[test]
    fn every_generated_vocabulary_term_is_typed() {
        let result = CoreVocabulary::try_from_generated();

        assert!(result.is_ok());
    }
}
