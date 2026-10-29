//! The receiver of a test's HTTP notifications.
//!
//! Each test starts its own sink on a port of its own, so nothing another test triggers ever reaches it. The sink
//! answers every `POST` under `/notify/` with 200 and keeps every request it receives, in arrival order; a test reads
//! the k-th notification of one endpoint, oldest first, as the ETSI notification keywords return them. A sink may
//! start refusing connections, as a notification server that has not started yet does.

use crate::{
    body_pattern::BodyPattern,
    callback_server::{CallbackError, CallbackServer},
    eventually::{Arrivals, Poll, TimedOut, delivery_deadline, eventually},
    header_condition::HeaderCondition,
};
use bytes::Bytes;
use http::HeaderMap;
use ngsi_ld::{
    http::ResponseHead,
    model::{DocumentError, Entity, Lossless, Output, SimplifiedEntity},
    registration::{CSourceNotification, Problem},
    subscription::{Notification, ReadErrorKind},
};
use std::{
    convert::Infallible,
    fmt::Display,
    str::{self, Utf8Error},
    time::Duration,
};
use tokio::time::sleep;
use url::Url;
use wiremock::{
    Mock,
    ResponseTemplate,
    matchers::{method, path, path_regex},
};

/// Why a notification could not be read.
#[derive(Debug, thiserror::Error)]
pub enum SinkError {
    /// The server could not start or does not serve yet.
    #[error(transparent)]
    Server(#[from] CallbackError),
    /// The awaited notification did not arrive.
    #[error(transparent)]
    TimedOut(#[from] TimedOut<Arrivals>),
}

/// Why a received body is not a notification.
#[derive(Debug, thiserror::Error)]
pub enum NotificationError {
    /// The body is not UTF-8 text.
    #[error(transparent)]
    Text(#[from] Utf8Error),
    /// The text is not a notification of the representation asked for.
    #[error(transparent)]
    Document(#[from] DocumentError<ReadErrorKind<Infallible>>),
    /// The text is not a context source notification.
    #[error(transparent)]
    Registrations(#[from] DocumentError<ReadErrorKind<Problem>>),
}

/// A notification as the sink received it.
#[derive(Debug, Clone)]
pub struct ReceivedNotification {
    /// Its header fields; names compare without regard to case.
    pub headers: HeaderMap,
    /// Its body, as sent.
    pub body: Bytes,
}

impl ReceivedNotification {
    /// Its header fields, read through the typed headers a response is read with.
    #[must_use]
    pub fn head(&self) -> ResponseHead {
        // Clone: the head owns its fields, and the sink keeps the notification.
        ResponseHead::new(self.headers.clone())
    }

    /// The body as a JSON tree, for an exact comparison.
    ///
    /// # Errors
    ///
    /// Returns the parse error when the body is not JSON.
    pub fn json(&self) -> Result<serde_json::Value, serde_json::Error> {
        serde_json::from_slice(&self.body)
    }

    /// The body read as a notification of entities in normalized or concise representation (clause 5.3.1).
    ///
    /// # Errors
    ///
    /// Returns [`NotificationError`] when the body is not such a notification.
    pub fn entities(&self, form: Lossless) -> Result<Notification<Vec<Entity<Output>>>, NotificationError> {
        Ok(Notification::<Vec<Entity<Output>>>::parse(str::from_utf8(&self.body)?, form)?)
    }

    /// The body read as a context source notification (clause 5.3.2): the registrations a Context Source
    /// Registration Subscription reports.
    ///
    /// # Errors
    ///
    /// Returns [`NotificationError`] when the body is not such a notification.
    pub fn registrations(&self) -> Result<CSourceNotification, NotificationError> {
        Ok(CSourceNotification::parse(str::from_utf8(&self.body)?)?)
    }

    /// The body read as a notification of entities in simplified representation (clause 5.3.1).
    ///
    /// # Errors
    ///
    /// Returns [`NotificationError`] when the body is not such a notification.
    pub fn simplified(&self) -> Result<Notification<Vec<SimplifiedEntity>>, NotificationError> {
        Ok(Notification::<Vec<SimplifiedEntity>>::parse(str::from_utf8(&self.body)?)?)
    }
}

/// A notification receiver of one test.
#[derive(Debug)]
pub struct NotificationSink {
    /// The server the brokers post to.
    server: CallbackServer,
}

impl NotificationSink {
    /// A sink that serves from the start.
    ///
    /// # Errors
    ///
    /// Returns [`SinkError`] when the server cannot start.
    pub async fn start() -> Result<NotificationSink, SinkError> {
        NotificationSink::serving(CallbackServer::start().await?).await
    }

    /// A sink that closes every connection unanswered until [`start_serving`](Self::start_serving).
    ///
    /// # Errors
    ///
    /// Returns [`SinkError`] when the server cannot listen.
    pub fn refusing() -> Result<NotificationSink, SinkError> {
        Ok(NotificationSink {
            server: CallbackServer::refusing()?,
        })
    }

    /// The sink answering from now on, with nothing recorded of what it refused.
    ///
    /// # Errors
    ///
    /// Returns [`SinkError`] when the server cannot start.
    pub async fn start_serving(self) -> Result<NotificationSink, SinkError> {
        NotificationSink::serving(self.server.start_serving().await).await
    }

    /// The sink over `server`, with its catch-all answer mounted.
    async fn serving(server: CallbackServer) -> Result<NotificationSink, SinkError> {
        Mock::given(method("POST"))
            .and(path_regex("^/notify/"))
            .respond_with(ResponseTemplate::new(200))
            .with_priority(10)
            .mount(server.server()?)
            .await;
        Ok(NotificationSink { server })
    }

    /// The URL of endpoint `n`, as the brokers reach it: `http://<callback host>:<port>/notify/<n>`.
    ///
    /// # Errors
    ///
    /// Returns [`SinkError::Server`] when `n` breaks the URL.
    pub fn endpoint(&self, n: impl Display) -> Result<Url, SinkError> {
        Ok(self.server.url(&format!("notify/{n}"))?)
    }

    /// Mounts `mock` in front of the catch-all answer, for an endpoint that must answer otherwise.
    ///
    /// # Errors
    ///
    /// Returns [`SinkError::Server`] before the sink serves.
    pub async fn mount(&self, mock: Mock) -> Result<(), SinkError> {
        self.server.server()?.register(mock).await;
        Ok(())
    }

    /// Answers every notification posted to endpoint `n` with `answer` instead of the catch-all 200, for an endpoint
    /// that fails, refuses or answers late.
    ///
    /// # Errors
    ///
    /// Returns [`SinkError::Server`] before the sink serves.
    pub async fn answer(&self, n: impl Display, answer: ResponseTemplate) -> Result<(), SinkError> {
        self.mount(
            Mock::given(method("POST"))
                .and(path(format!("/notify/{n}")))
                .respond_with(answer)
                .with_priority(1),
        )
        .await
    }

    /// Every notification endpoint `n` received so far, oldest first.
    ///
    /// # Errors
    ///
    /// Returns [`SinkError::Server`] before the sink serves.
    pub async fn received(&self, n: impl Display) -> Result<Vec<ReceivedNotification>, SinkError> {
        let path = format!("/notify/{n}");
        Ok(self
            .server
            .received()
            .await?
            .into_iter()
            .filter(|request| request.url.path() == path)
            .map(|request| ReceivedNotification {
                headers: request.headers,
                body: Bytes::from(request.body),
            })
            .collect())
    }

    /// Every notification endpoint `n` received so far whose body meets every one of `patterns`, oldest first.
    ///
    /// # Errors
    ///
    /// Returns [`SinkError::Server`] before the sink serves.
    pub async fn received_where(&self, n: impl Display, patterns: &[BodyPattern]) -> Result<Vec<ReceivedNotification>, SinkError> {
        self.received_matching(n, patterns, &[]).await
    }

    /// Every notification endpoint `n` received so far whose body meets every one of `patterns` and whose header
    /// fields meet every one of `headers`, oldest first.
    ///
    /// # Errors
    ///
    /// Returns [`SinkError::Server`] before the sink serves.
    pub async fn received_matching(
        &self,
        n: impl Display,
        patterns: &[BodyPattern],
        headers: &[HeaderCondition],
    ) -> Result<Vec<ReceivedNotification>, SinkError> {
        Ok(self
            .received(n)
            .await?
            .into_iter()
            .filter(|notification| {
                patterns.iter().all(|pattern| pattern.holds(&notification.body)) && headers.iter().all(|condition| condition.holds_in(&notification.headers))
            })
            .collect())
    }

    /// Every notification endpoint `n` received whose body meets every one of `patterns`, oldest first, once it
    /// holds at least `k` of them, waiting up to the [`delivery_deadline`].
    ///
    /// # Errors
    ///
    /// Returns [`SinkError::TimedOut`] when fewer than `k` arrive within the deadline.
    pub async fn wait_for_at_least_where(
        &self,
        n: impl Display + Copy + Send + Sync,
        patterns: &[BodyPattern],
        k: usize,
    ) -> Result<Vec<ReceivedNotification>, SinkError> {
        self.wait_for_at_least_matching(n, patterns, &[], k).await
    }

    /// Every notification endpoint `n` received whose body meets every one of `patterns` and whose header fields
    /// meet every one of `headers`, oldest first, once it holds at least `k` of them, waiting up to the
    /// [`delivery_deadline`].
    ///
    /// # Errors
    ///
    /// Returns [`SinkError::TimedOut`] when fewer than `k` arrive within the deadline.
    pub async fn wait_for_at_least_matching(
        &self,
        n: impl Display + Copy + Send + Sync,
        patterns: &[BodyPattern],
        headers: &[HeaderCondition],
        k: usize,
    ) -> Result<Vec<ReceivedNotification>, SinkError> {
        eventually(delivery_deadline(), || async move {
            match self.received_matching(n, patterns, headers).await {
                Ok(received) if received.len() >= k => Poll::Ready(Ok(received)),
                Ok(received) => Poll::Pending(Arrivals {
                    held: received.len(),
                    awaited: k,
                }),
                Err(error) => Poll::Ready(Err(error)),
            }
        })
        .await?
    }

    /// The k-th notification (1 for the first) endpoint `n` receives, waiting up to the [`delivery_deadline`] for it.
    ///
    /// # Errors
    ///
    /// Returns [`SinkError::TimedOut`] when fewer than `k` arrive within the deadline.
    pub async fn wait_for(&self, n: impl Display + Copy + Send + Sync, k: usize) -> Result<ReceivedNotification, SinkError> {
        eventually(delivery_deadline(), || async move {
            match self.received(n).await {
                Ok(mut received) if received.len() >= k => Poll::Ready(Ok(received.swap_remove(k - 1))),
                Ok(received) => Poll::Pending(Arrivals {
                    held: received.len(),
                    awaited: k,
                }),
                Err(error) => Poll::Ready(Err(error)),
            }
        })
        .await?
    }

    /// Every notification endpoint `n` received, oldest first, once it holds at least `k`, waiting up to the
    /// [`delivery_deadline`] for them.
    ///
    /// # Errors
    ///
    /// Returns [`SinkError::TimedOut`] when fewer than `k` arrive within the deadline.
    pub async fn wait_for_at_least(&self, n: impl Display + Copy + Send + Sync, k: usize) -> Result<Vec<ReceivedNotification>, SinkError> {
        eventually(delivery_deadline(), || async move {
            match self.received(n).await {
                Ok(received) if received.len() >= k => Poll::Ready(Ok(received)),
                Ok(received) => Poll::Pending(Arrivals {
                    held: received.len(),
                    awaited: k,
                }),
                Err(error) => Poll::Ready(Err(error)),
            }
        })
        .await?
    }

    /// The number of notifications endpoint `n` holds after waiting `window` in full: an absence check, which the
    /// test compares with the count its earlier waits took.
    ///
    /// # Errors
    ///
    /// Returns [`SinkError::Server`] before the sink serves.
    pub async fn count_after(&self, n: impl Display, window: Duration) -> Result<usize, SinkError> {
        sleep(window).await;
        Ok(self.received(n).await?.len())
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        header_condition::HeaderCondition,
        notification_sink::{NotificationSink, ReceivedNotification},
    };
    use bytes::Bytes;
    use googletest::prelude::*;
    use http::{HeaderMap, HeaderName, HeaderValue, header::CONTENT_TYPE};
    use ngsi_ld::{
        http::headers::ContentType,
        model::{Lossless, NgsiLdMediaType},
        registration::TriggerReason,
    };
    use wiremock::ResponseTemplate;

    /// A notification received with `body`.
    fn received(body: &'static str) -> ReceivedNotification {
        ReceivedNotification {
            headers: HeaderMap::new(),
            body: Bytes::from_static(body.as_bytes()),
        }
    }

    #[gtest]
    fn the_head_reads_the_content_type_through_the_typed_header() -> Result<()> {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/ld+json"));
        let notification = ReceivedNotification {
            headers,
            body: Bytes::from_static(b"{}"),
        };
        verify_that!(
            notification.head().typed::<ContentType>().valid(),
            some(eq(&ContentType::NgsiLd(NgsiLdMediaType::JsonLd)))
        )
    }

    #[gtest]
    fn a_context_source_notification_reads_its_registrations() -> Result<()> {
        let notification = received(concat!(
            r#"{"id":"urn:ngsi-ld:Notification:1","type":"ContextSourceNotification","subscriptionId":"urn:ngsi-ld:Subscription:1","#,
            r#""notifiedAt":"2024-01-01T00:00:00Z","triggerReason":"newlyMatching","data":[{"id":"urn:ngsi-ld:ContextSourceRegistration:1","#,
            r#""type":"ContextSourceRegistration","information":[{"entities":[{"type":"Building"}]}],"endpoint":"http://source.test/"}]}"#
        ))
        .registrations()?;
        expect_that!(notification.trigger_reason(), eq(TriggerReason::NewlyMatching));
        verify_that!(notification.data(), len(eq(1)))
    }

    #[gtest]
    fn an_entity_notification_is_not_a_context_source_notification() -> Result<()> {
        let notification = received(concat!(
            r#"{"id":"urn:ngsi-ld:Notification:1","type":"Notification","subscriptionId":"urn:ngsi-ld:Subscription:1","#,
            r#""notifiedAt":"2024-01-01T00:00:00Z","data":[{"id":"urn:ngsi-ld:Building:1","type":"Building"}]}"#
        ));
        expect_that!(notification.entities(Lossless::Normalized)?.data().as_slice(), len(eq(1)));
        verify_that!(notification.registrations().is_err(), eq(true))
    }

    #[gtest]
    #[tokio::test(flavor = "multi_thread")]
    async fn a_read_matching_header_fields_keeps_the_notifications_meeting_them() -> Result<()> {
        let sink = NotificationSink::start().await?;
        let target = format!("{}/notify/1", sink.server.server()?.uri());
        let client = reqwest::Client::new();
        client.post(&target).header("ngsild-tenant", "t1").body("{}").send().await?;
        client.post(&target).body("{}").send().await?;
        let tenant = HeaderName::from_static("ngsild-tenant");
        expect_that!(
            sink.received_matching(1, &[], &[HeaderCondition::Is(tenant.clone(), "t1".to_owned())]).await?,
            len(eq(1))
        );
        expect_that!(sink.received_matching(1, &[], &[HeaderCondition::Absent(tenant.clone())]).await?, len(eq(1)));
        verify_that!(
            sink.wait_for_at_least_matching(1, &[], &[HeaderCondition::Is(tenant, "t2".to_owned())], 0)
                .await?,
            is_empty()
        )
    }

    #[gtest]
    #[tokio::test(flavor = "multi_thread")]
    async fn an_endpoint_given_its_own_answer_answers_it_and_still_records_the_notification() -> Result<()> {
        let sink = NotificationSink::start().await?;
        sink.answer(2, ResponseTemplate::new(500)).await?;
        let root = sink.server.server()?.uri();
        let client = reqwest::Client::new();
        let failing = client.post(format!("{root}/notify/2")).body("{}").send().await?;
        let answered = client.post(format!("{root}/notify/3")).body("{}").send().await?;
        expect_that!(failing.status().as_u16(), eq(500));
        expect_that!(answered.status().as_u16(), eq(200));
        verify_that!(sink.received(2).await?, len(eq(1)))
    }
}
