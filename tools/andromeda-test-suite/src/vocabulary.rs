//! The `@context` of an ETSI test: every term the test uses mapped to an IRI of its own namespace.
//!
//! The ETSI material fixes its term names (`Building`, `speed`), and a shared term would expand to a shared IRI. Each
//! test therefore stores a context of the suite's compound shape, a term object followed by the versioned core
//! `@context`, mapping each term to `https://test.andromeda/<token>/<term>`. A broker appends the core `@context` to
//! a user `@context` that lacks it, so the core reference is part of the stored document: a body that echoes the
//! context then echoes the stored URL alone, as it does for the suite's compound document.

use crate::{
    broker::BrokerClient,
    config::{ConfigError, SuiteConfig},
    namespace::{Namespace, NamespaceError},
};
use iri_rs::{IriBuf, IriRefBuf};
use ngsi_ld::{
    client::{ClientError, ReqwestError},
    model::{Context, ContextEntry, InlineContext, RawJson},
};
use std::collections::BTreeMap;
use url::Url;

/// Why the vocabulary could not be stored.
#[derive(Debug, thiserror::Error)]
pub enum VocabularyError {
    /// The configuration names no core `@context`.
    #[error(transparent)]
    Config(#[from] ConfigError),
    /// A term breaks its IRI.
    #[error(transparent)]
    Namespace(#[from] NamespaceError),
    /// The term object could not be written.
    #[error("the term object could not be written: {0}")]
    Terms(String),
    /// The broker refused to store the context.
    #[error("the broker refused the vocabulary")]
    Refused(#[source] Box<ClientError<ReqwestError>>),
    /// The broker named no Location for the stored context.
    #[error("the broker answered the vocabulary with no readable Location")]
    NoLocation,
    /// The Location does not resolve against the broker URL.
    #[error("the Location {0} does not resolve against the broker")]
    Location(String, #[source] url::ParseError),
}

/// A stored test vocabulary.
#[derive(Debug, Clone)]
pub struct Vocabulary {
    /// The URL the broker serves the context at.
    url: IriBuf,
}

impl Vocabulary {
    /// Stores the vocabulary mapping `terms` under `namespace`, on the broker and tenant `client` speaks to.
    ///
    /// # Errors
    ///
    /// Returns [`VocabularyError::Refused`] when the broker answers anything but 201.
    pub async fn register(client: &BrokerClient, namespace: &Namespace, terms: &[&str]) -> Result<Vocabulary, VocabularyError> {
        let config = SuiteConfig::get().map_err(|error| VocabularyError::Config(error.clone()))?;
        let mapping = terms
            .iter()
            .map(|term| namespace.vocabulary_iri(term).map(|iri| (*term, iri.into_inner())))
            .collect::<Result<BTreeMap<_, _>, _>>()?;
        let text = serde_json::to_string(&mapping).map_err(|error| VocabularyError::Terms(error.to_string()))?;
        let inline = text
            .parse::<RawJson>()
            .map_err(|error| VocabularyError::Terms(error.to_string()))
            .and_then(|json| InlineContext::try_from(json).map_err(|error| VocabularyError::Terms(error.to_string())))?;
        let core = IriRefBuf::new(config.core_context().as_str().to_owned()).map_err(|error| VocabularyError::Terms(error.to_string()))?;
        let context = Context::Multiple(vec![ContextEntry::Inline(inline), ContextEntry::Reference(core)]);
        let created = client
            .contexts()
            .add(&context)
            .send()
            .await
            .map_err(|error| VocabularyError::Refused(Box::new(error)))?;
        let location = created.location.valid().ok_or(VocabularyError::NoLocation)?;
        let broker = Url::parse(client.endpoint().base_url().as_str()).map_err(|error| VocabularyError::Location(location.as_str().to_owned(), error))?;
        let resolved = broker
            .join(location.as_str())
            .map_err(|error| VocabularyError::Location(location.as_str().to_owned(), error))?;
        let url = IriBuf::new(resolved.to_string()).map_err(|_| VocabularyError::NoLocation)?;
        Ok(Vocabulary { url })
    }

    /// The URL the broker serves the vocabulary at.
    #[must_use]
    pub const fn url(&self) -> &IriBuf {
        &self.url
    }

    /// The vocabulary as the `@context` of a request: its stored URL, as a string.
    #[must_use]
    pub fn context(&self) -> Context {
        // The context owns its reference; the vocabulary keeps its URL for later requests.
        Context::from(self.url.clone())
    }

    /// The vocabulary as the `@context` of a request written as a one-element list, as the suite's payload files
    /// write `"@context": ["<url>"]`.
    #[must_use]
    pub fn context_list(&self) -> Context {
        // The context owns its reference; the vocabulary keeps its URL for later requests.
        Context::Multiple(vec![ContextEntry::Reference(IriRefBuf::from(self.url.clone()))])
    }
}
