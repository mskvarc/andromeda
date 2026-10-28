//! `@context` documents a test serves for the broker to download.
//!
//! A broker caches a downloaded `@context` by its URL, so a URL on the test's own server is a cache entry only that
//! test reads, reloads or deletes. A path no document is served at answers 404, as the host of a missing document
//! does. Stopping the server closes its port, so every later download of its documents fails.

use crate::callback_server::{CallbackError, CallbackServer};
use iri_rs::IriRefBuf;
use ngsi_ld::model::ContextEntry;
use url::Url;
use wiremock::{
    Mock,
    ResponseTemplate,
    matchers::{method, path},
};

/// Why a served document cannot be named as a context entry.
#[derive(Debug, thiserror::Error)]
pub enum RemoteContextError {
    /// The path breaks the server's URL.
    #[error(transparent)]
    Server(#[from] CallbackError),
    /// The URL is not an IRI reference.
    #[error("the context URL {0} is not an IRI")]
    NotAnIri(Url),
}

/// A server of `@context` documents.
#[derive(Debug)]
pub struct RemoteContexts {
    /// The server the broker downloads from.
    server: CallbackServer,
}

impl RemoteContexts {
    /// A server serving nothing yet.
    ///
    /// # Errors
    ///
    /// Returns [`CallbackError`] when the server cannot start.
    pub async fn start() -> Result<RemoteContexts, CallbackError> {
        Ok(RemoteContexts {
            server: CallbackServer::start().await?,
        })
    }

    /// The URL of `path` (relative, without a leading `/`) as the brokers reach it.
    ///
    /// # Errors
    ///
    /// Returns [`CallbackError::Url`] when `path` breaks the URL.
    pub fn url(&self, path: &str) -> Result<Url, CallbackError> {
        self.server.url(path)
    }

    /// The `@context` entry referencing the document at `path` (relative, without a leading `/`).
    ///
    /// # Errors
    ///
    /// Returns [`RemoteContextError`] when `path` breaks the URL.
    pub fn reference(&self, path: &str) -> Result<ContextEntry, RemoteContextError> {
        let url = self.url(path)?;
        IriRefBuf::new(url.as_str().to_owned())
            .map(ContextEntry::Reference)
            .map_err(|_| RemoteContextError::NotAnIri(url))
    }

    /// Serves `document` at `at` (relative, without a leading `/`) with the `Content-Type` `media_type`, or with none when `media_type` is `None`.
    ///
    /// # Errors
    ///
    /// Returns [`CallbackError`] when `at` breaks the URL.
    pub async fn serve(&self, at: &str, document: &str, media_type: Option<&str>) -> Result<Url, CallbackError> {
        let url = self.url(at)?;
        let answer = match media_type {
            Some(media_type) => ResponseTemplate::new(200).set_body_raw(document.as_bytes().to_vec(), media_type),
            None => ResponseTemplate::new(200).set_body_bytes(document.as_bytes()),
        };
        Mock::given(method("GET"))
            .and(path(url.path()))
            .respond_with(answer)
            .mount(self.server.server()?)
            .await;
        Ok(url)
    }

    /// The number of downloads of `at` the server answered.
    ///
    /// # Errors
    ///
    /// Returns [`CallbackError`] when `at` breaks the URL.
    pub async fn downloads(&self, at: &str) -> Result<usize, CallbackError> {
        let url = self.url(at)?;
        Ok(self.server.received().await?.iter().filter(|request| request.url.path() == url.path()).count())
    }

    /// Stops serving: the port closes, and every later download fails.
    pub fn stop(self) {
        drop(self.server);
    }
}
