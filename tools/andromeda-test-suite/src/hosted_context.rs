//! An `@context` a test stores on a broker (Add @context, clause 6.29.3.1): its identifier and the URL the broker
//! serves it at (clause 5.13.2.4), which later requests name as a context reference.

use crate::broker::BrokerClient;
use iri_rs::IriRefBuf;
use ngsi_ld::{
    client::{ClientError, ReqwestError},
    model::{Context, ContextEntry, ContextId},
};
use percent_encoding::percent_decode_str;
use url::Url;

/// Why a context could not be stored or named.
#[derive(Debug, thiserror::Error)]
pub enum HostedContextError {
    /// The broker refused the context or the call failed.
    #[error("the broker did not store the context")]
    Add(#[source] Box<ClientError<ReqwestError>>),
    /// The answer carries no `Location` naming a stored context.
    #[error("the answer names no stored context")]
    NoLocation,
    /// The `Location` does not resolve against the broker's API root.
    #[error("the location {0} does not resolve against the broker")]
    Location(IriRefBuf, #[source] url::ParseError),
    /// The served URL is not an IRI reference.
    #[error("the context URL {0} is not an IRI")]
    NotAnIri(Url),
    /// A listing names no URL holding the text sought, or several.
    #[error("the listing names {holding} URLs holding the text sought, not one: {listed:?}")]
    NotListedOnce {
        /// How many listed URLs hold the text.
        holding: usize,
        /// The URLs the listing names.
        listed: Vec<Url>,
    },
    /// The URL's last path segment is not the identifier of a stored context.
    #[error("the listed URL {0} names no stored context")]
    NotStored(Url),
}

/// A context stored on a broker.
#[derive(Debug, Clone)]
pub struct HostedContext {
    /// The identifier the broker gave it.
    pub id: ContextId,
    /// The URL the broker serves it at, as the brokers of the stack reach it.
    pub url: Url,
    /// The reference to it as the broker wrote it, relative to the broker's origin for Add @context's `Location`
    /// and the listed URL's path for a listed context.
    pub location: String,
}

impl HostedContext {
    /// Stores `context` through `client`, in the client's tenant.
    ///
    /// # Errors
    ///
    /// Returns [`HostedContextError`] when the broker refuses it or names no stored context.
    pub async fn add(client: &BrokerClient, context: &Context) -> Result<HostedContext, HostedContextError> {
        let created = client
            .contexts()
            .add(context)
            .send()
            .await
            .map_err(|error| HostedContextError::Add(Box::new(error)))?;
        let location = created.location.valid().ok_or(HostedContextError::NoLocation)?;
        let id = created.id().ok_or(HostedContextError::NoLocation)?;
        let url = client
            .endpoint()
            .base_url()
            .join(location.as_str())
            // clone: the answer keeps its location; the error owns its own copy
            .map_err(|error| HostedContextError::Location(location.clone(), error))?;
        Ok(HostedContext {
            id,
            url,
            location: location.as_str().to_owned(),
        })
    }

    /// The stored context of the one URL of `listed` holding `text`, as List @contexts names it (clause 6.29.3.2):
    /// its identifier is the last path segment of that URL (clause 5.13.2.4), percent-decoded.
    ///
    /// # Errors
    ///
    /// Returns [`HostedContextError::NotListedOnce`] unless exactly one URL holds `text`, and
    /// [`HostedContextError::NotStored`] when its last segment is no context identifier.
    pub fn listed(listed: &[Url], text: &str) -> Result<HostedContext, HostedContextError> {
        let mut holding = listed.iter().filter(|url| url.as_str().contains(text));
        let (Some(url), None) = (holding.next(), holding.next()) else {
            return Err(HostedContextError::NotListedOnce {
                holding: listed.iter().filter(|url| url.as_str().contains(text)).count(),
                // The listing keeps its URLs; the error owns its own copy.
                listed: listed.to_vec(),
            });
        };
        let id = url
            .path_segments()
            .and_then(Iterator::last)
            .and_then(|segment| percent_decode_str(segment).decode_utf8().ok())
            .and_then(|segment| segment.parse::<ContextId>().ok())
            .ok_or_else(|| HostedContextError::NotStored(url.clone()))?;
        // The listing keeps its URLs; the stored context owns its own copy.
        Ok(HostedContext {
            id,
            url: url.clone(),
            location: url.path().to_owned(),
        })
    }

    /// The `@context` entry referencing the stored context by its `location`, as written: a reference relative to the
    /// broker's origin when the broker answered one.
    ///
    /// # Errors
    ///
    /// Returns [`HostedContextError::NotAnIri`] when the location is not an IRI reference.
    pub fn relative_entry(&self) -> Result<ContextEntry, HostedContextError> {
        IriRefBuf::new(self.location.clone())
            .map(ContextEntry::Reference)
            .map_err(|_| HostedContextError::NotAnIri(self.url.clone()))
    }

    /// The `@context` entry referencing the stored context by its served URL.
    ///
    /// # Errors
    ///
    /// Returns [`HostedContextError::NotAnIri`] when the URL is not an IRI reference.
    pub fn entry(&self) -> Result<ContextEntry, HostedContextError> {
        IriRefBuf::new(self.url.as_str().to_owned())
            .map(ContextEntry::Reference)
            .map_err(|_| HostedContextError::NotAnIri(self.url.clone()))
    }
}

#[cfg(test)]
mod tests {
    use crate::hosted_context::HostedContext;
    use googletest::prelude::*;
    use url::Url;

    #[gtest]
    fn the_one_listed_url_of_a_stored_context_names_its_identifier() -> Result<()> {
        let listed = [
            Url::parse("https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld")?,
            Url::parse("http://broker:1056/ngsi-ld/v1/jsonldContexts/urn%3Angsi-ld%3AJsonLdContext%3Aa1")?,
        ];
        let stored = HostedContext::listed(&listed, "JsonLdContext")?;
        expect_that!(stored.id.to_string(), eq("urn:ngsi-ld:JsonLdContext:a1"));
        verify_that!(stored.url.as_str(), ends_with("JsonLdContext%3Aa1"))
    }

    #[gtest]
    fn a_text_no_url_or_several_urls_hold_names_no_stored_context() -> Result<()> {
        let listed = [
            Url::parse("http://broker/ngsi-ld/v1/jsonldContexts/urn:ngsi-ld:JsonLdContext:a")?,
            Url::parse("http://broker/ngsi-ld/v1/jsonldContexts/urn:ngsi-ld:JsonLdContext:b")?,
        ];
        expect_that!(HostedContext::listed(&listed, "JsonLdContext").is_err(), eq(true));
        verify_that!(HostedContext::listed(&listed, "Other").is_err(), eq(true))
    }
}
