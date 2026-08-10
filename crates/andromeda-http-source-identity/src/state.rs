//! Focused immutable state for context source identity retrieval.

use andromeda_ngsi_ld_lexicon::{context_source_pseudonym::ContextSourcePseudonym, uri::Uri};
use andromeda_ngsi_ld_model::context_source_identity::{ContextSourceIdentity, ContextSourceIdentityType};
use std::sync::Arc;

/// State containing only the identity document exposed by this binding.
#[derive(Clone)]
pub struct SourceIdentityState {
    /// Immutable identity shared by every request.
    identity: Arc<ContextSourceIdentity>,
}

impl SourceIdentityState {
    /// Construct the context source identity exposed by the broker.
    pub fn new(id: Uri, alias: ContextSourcePseudonym) -> SourceIdentityState {
        SourceIdentityState {
            identity: Arc::new(ContextSourceIdentity {
                id,
                type_: ContextSourceIdentityType,
                context_source_alias: alias,
            }),
        }
    }

    /// Borrow the immutable identity document.
    pub fn identity(&self) -> &ContextSourceIdentity {
        &self.identity
    }
}
