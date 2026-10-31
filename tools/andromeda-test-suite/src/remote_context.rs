//! `@context` documents a test serves for the broker to download.
//!
//! A broker caches a downloaded `@context` by its URL, so a URL on the test's own server is a cache entry only that
//! test reads, reloads or deletes. A path no document is served at answers 404, as the host of a missing document
//! does. Stopping the server closes its port, so every later download of its documents fails.

use crate::callback_server::{CallbackError, CallbackServer};
use http::{HeaderName, HeaderValue, StatusCode};
use iri_rs::IriRefBuf;
use ngsi_ld::model::ContextEntry;
use std::sync::atomic::{AtomicU8, Ordering};
use url::Url;
use wiremock::{
    Mock,
    ResponseTemplate,
    matchers::{method, path, path_regex},
};

/// The answer a test's server gives for one document: its status, media type and the response headers beside the
/// body, such as the caching directives a broker's download honours (RFC 9111 section 5.2).
#[derive(Debug, Clone)]
pub struct ServedAnswer {
    /// The status the document is answered with.
    pub status: StatusCode,
    /// The media type of the body, written as the `Content-Type` of the answer.
    pub media_type: Option<&'static str>,
    /// The response headers beside the body, in the order given.
    pub headers: Vec<(HeaderName, HeaderValue)>,
}

impl ServedAnswer {
    /// The document is answered 200 with no header beside the body.
    #[must_use]
    pub const fn ok() -> ServedAnswer {
        ServedAnswer {
            status: StatusCode::OK,
            media_type: None,
            headers: Vec::new(),
        }
    }

    /// The document is answered with `status`.
    #[must_use]
    pub fn with_status(self, status: StatusCode) -> ServedAnswer {
        ServedAnswer { status, ..self }
    }

    /// The body is answered under `media_type`.
    #[must_use]
    pub fn with_media_type(self, media_type: &'static str) -> ServedAnswer {
        ServedAnswer {
            media_type: Some(media_type),
            ..self
        }
    }

    /// The answer carries the response header `name` with `value`.
    #[must_use]
    pub fn with_header(mut self, name: HeaderName, value: HeaderValue) -> ServedAnswer {
        self.headers.push((name, value));
        self
    }

    /// The mock server answer for `document`.
    fn template(self, document: &str) -> ResponseTemplate {
        let body = document.as_bytes().to_vec();
        let template = match self.media_type {
            Some(media_type) => ResponseTemplate::new(self.status).set_body_raw(body, media_type),
            None => ResponseTemplate::new(self.status).set_body_bytes(body),
        };
        template.append_headers(self.headers)
    }
}

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
    /// How many documents the server has served, so a later one outranks an earlier one.
    served: AtomicU8,
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
            served: AtomicU8::new(0),
        })
    }

    /// The URL of `path` (relative, without a leading `/`) as the brokers reach it.
    ///
    /// # Errors
    ///
    /// Returns [`CallbackError::Join`] when `path` breaks the URL.
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
    pub async fn serve(&self, at: &str, document: &str, media_type: Option<&'static str>) -> Result<Url, CallbackError> {
        let answer = match media_type {
            Some(media_type) => ServedAnswer::ok().with_media_type(media_type),
            None => ServedAnswer::ok(),
        };
        self.serve_answer(at, document, answer).await
    }

    /// Serves `document` at `at` (relative, without a leading `/`) answered as `answer` states, with its status,
    /// media type and response headers.
    ///
    /// A document served later at a path the server already answers for replaces the earlier answer: the test
    /// changes what the path holds.
    ///
    /// # Errors
    ///
    /// Returns [`CallbackError`] when `at` breaks the URL.
    pub async fn serve_answer(&self, at: &str, document: &str, answer: ServedAnswer) -> Result<Url, CallbackError> {
        self.mount(at, document, answer, None).await
    }

    /// Serves `first_document` answered as `first_answer` states to the first download of `at`, and
    /// `then_document` answered as `then_answer` states to every later one: a document that changes once it was read.
    ///
    /// # Errors
    ///
    /// Returns [`CallbackError`] when `at` breaks the URL.
    pub async fn serve_then(
        &self,
        at: &str,
        (first_document, first_answer): (&str, ServedAnswer),
        (then_document, then_answer): (&str, ServedAnswer),
    ) -> Result<Url, CallbackError> {
        self.mount(at, then_document, then_answer, None).await?;
        self.mount(at, first_document, first_answer, Some(1)).await
    }

    /// Serves `document` answered as `answer` states at every path that starts with `prefix` (relative, without a
    /// leading `/`): the documents of a test that names a download URL of its own for each of many requests.
    ///
    /// # Errors
    ///
    /// Returns [`CallbackError`] when `prefix` breaks the URL.
    pub async fn serve_all_under(&self, prefix: &str, document: &str, answer: ServedAnswer) -> Result<Url, CallbackError> {
        let url = self.url(prefix)?;
        Mock::given(method("GET"))
            .and(path_regex(format!("^{}.*$", regex::escape(url.path()))))
            .respond_with(answer.template(document))
            .mount(self.server.server()?)
            .await;
        Ok(url)
    }

    /// Mounts `document` at `at`, answered `limit` times at most when a limit is given.
    async fn mount(&self, at: &str, document: &str, answer: ServedAnswer, limit: Option<u64>) -> Result<Url, CallbackError> {
        let url = self.url(at)?;
        // A later document outranks the earlier one at the same path, so the test replaces what the path serves.
        let priority = u8::MAX.saturating_sub(self.served.fetch_add(1, Ordering::Relaxed));
        let mock = Mock::given(method("GET"))
            .and(path(url.path()))
            .respond_with(answer.template(document))
            .with_priority(priority);
        let mock = match limit {
            Some(times) => mock.up_to_n_times(times),
            None => mock,
        };
        mock.mount(self.server.server()?).await;
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

#[cfg(test)]
mod tests {
    use crate::remote_context::{RemoteContexts, ServedAnswer};
    use googletest::prelude::*;
    use http::{StatusCode, header::CACHE_CONTROL};

    #[gtest]
    fn a_served_answer_holds_the_status_media_type_and_headers_it_is_given() -> Result<()> {
        let answer = ServedAnswer::ok()
            .with_status(StatusCode::FOUND)
            .with_media_type("text/html")
            .with_header(CACHE_CONTROL, "no-store".parse()?);
        expect_that!(answer.status, eq(StatusCode::FOUND));
        expect_that!(answer.media_type, some(eq("text/html")));
        verify_that!(answer.headers, elements_are![eq(&(CACHE_CONTROL, "no-store".parse()?))])
    }

    #[gtest]
    fn a_default_served_answer_is_an_ok_without_headers() -> Result<()> {
        let answer = ServedAnswer::ok();
        expect_that!(answer.status, eq(StatusCode::OK));
        expect_that!(answer.media_type, none());
        verify_that!(answer.headers, is_empty())
    }

    #[gtest]
    #[tokio::test]
    async fn a_context_server_serves_a_path_at_its_own_url() -> Result<()> {
        let contexts = RemoteContexts::start().await?;
        let url = contexts.serve("jc/a/terms.jsonld", r#"{"@context": {}}"#, Some("application/ld+json")).await?;
        verify_that!(url.path(), eq("/jc/a/terms.jsonld"))
    }

    #[gtest]
    #[tokio::test]
    async fn a_document_served_then_changes_after_its_first_download() -> Result<()> {
        let contexts = RemoteContexts::start().await?;
        let url = contexts
            .serve_then(
                "jc/t/terms.jsonld",
                (r#"{"name": "first"}"#, ServedAnswer::ok()),
                (r#"{"name": "later"}"#, ServedAnswer::ok()),
            )
            .await?;
        let first = reqwest::get(url.clone()).await?.text().await?;
        let second = reqwest::get(url.clone()).await?.text().await?;
        let third = reqwest::get(url).await?.text().await?;
        expect_that!(first, eq(r#"{"name": "first"}"#));
        expect_that!(second, eq(r#"{"name": "later"}"#));
        verify_that!(third, eq(r#"{"name": "later"}"#))
    }

    #[gtest]
    #[tokio::test]
    async fn a_document_served_under_a_prefix_answers_every_path_below_it() -> Result<()> {
        let contexts = RemoteContexts::start().await?;
        let prefix = contexts.serve_all_under("jc/p/", r#"{"name": "any"}"#, ServedAnswer::ok()).await?;
        let below = contexts.url("jc/p/many-1")?;
        let outside = contexts.url("jc/q/many-1")?;
        expect_that!(prefix.path(), eq("/jc/p/"));
        expect_that!(reqwest::get(below).await?.text().await?, eq(r#"{"name": "any"}"#));
        verify_that!(reqwest::get(outside).await?.status().as_u16(), eq(404))
    }

    #[gtest]
    #[tokio::test]
    async fn a_later_document_replaces_the_earlier_one_at_its_path() -> Result<()> {
        let contexts = RemoteContexts::start().await?;
        contexts.serve("jc/r/terms.jsonld", r#"{"name": "first"}"#, None).await?;
        let url = contexts
            .serve_answer(
                "jc/r/terms.jsonld",
                r#"{"name": "second"}"#,
                ServedAnswer::ok().with_media_type("application/json"),
            )
            .await?;
        let body = reqwest::get(url.clone()).await?.text().await?;
        verify_that!(body, eq(r#"{"name": "second"}"#))
    }
}
