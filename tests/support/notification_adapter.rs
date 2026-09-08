//! JSON notification adapter over the shared raw callback server.

use andromeda_test_mocks::{callback_server::CallbackServerError, notification_server::NotificationServer, server_config::MockServerConfig};
use axum::http::HeaderMap;
use serde_json::Value;
use std::time::Duration;
use tokio::{sync::Mutex, time::Instant};

/// A single notification request captured by the mock callback server.
pub struct CapturedNotification {
    /// HTTP headers the broker sent with the notification.
    pub headers: HeaderMap,
    /// Parsed JSON body of the notification.
    pub body: Value,
}

/// Mock notification endpoint that records the notifications a broker delivers to it.
pub struct NotificationAdapter {
    server: NotificationServer,
    pending: Mutex<Vec<CapturedNotification>>,
}

impl NotificationAdapter {
    /// Starts a notification server bound to the mock server address from the environment.
    ///
    /// # Panics
    ///
    /// Panics when the mock server configuration cannot be read from the environment, or when
    /// the notification server fails to bind its listening socket.
    pub async fn start() -> NotificationAdapter {
        let configuration = MockServerConfig::from_environment().expect("valid notification server configuration");
        let server = NotificationServer::start(configuration).await.expect("notification server starts");
        NotificationAdapter {
            server,
            pending: Mutex::new(Vec::new()),
        }
    }

    /// Returns the callback URL that subscriptions should notify.
    ///
    /// # Panics
    ///
    /// Panics when the server's bound address cannot be turned into an endpoint URL.
    pub fn endpoint_url(&self) -> String {
        self.server.endpoint_url().expect("notification endpoint resolves")
    }

    /// Discards every notification captured so far, both buffered and still queued on the server.
    pub async fn clear_notifications(&self) {
        self.pending.lock().await.clear();
        let _drained = self.server.drain_requests().await;
    }

    /// Waits for the next notification of any kind.
    ///
    /// # Panics
    ///
    /// Panics when no notification arrives within `timeout`, or when the received body is not
    /// valid JSON.
    pub async fn wait_for_notification(&self, timeout: Duration) -> CapturedNotification {
        let request = self.server.next_request(timeout).await.expect("notification arrives before timeout");
        let body = request.json().expect("notification body is valid JSON");
        CapturedNotification {
            headers: request.headers,
            body,
        }
    }

    /// Waits for the next notification carrying the given `subscriptionId`.
    ///
    /// # Panics
    ///
    /// Panics when no matching notification arrives within `timeout`, or when a received body is
    /// not valid JSON.
    pub async fn wait_for_subscription_notification(&self, subscription_id: &str, timeout: Duration) -> CapturedNotification {
        self.wait_for_notification_matching("subscriptionId", subscription_id, timeout).await
    }

    /// Waits for the next notification whose top-level `member` equals `value`, buffering any
    /// non-matching notifications for later inspection.
    ///
    /// # Panics
    ///
    /// Panics when no matching notification arrives within `timeout`, or when a received body is
    /// not valid JSON.
    pub async fn wait_for_notification_matching(&self, member: &str, value: &str, timeout: Duration) -> CapturedNotification {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(notification) = self.take_matching(member, value).await {
                return notification;
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            let request = self.server.next_request(remaining).await.expect("notification arrives before timeout");
            let body = request.json().expect("notification body is valid JSON");
            self.pending.lock().await.push(CapturedNotification {
                headers: request.headers,
                body,
            });
        }
    }

    /// Asserts that no notification whose top-level `member` equals `value` arrives within
    /// `duration`.
    ///
    /// # Panics
    ///
    /// Panics when a matching notification arrives, when a received body is not valid JSON, or
    /// when the callback server fails for a reason other than the expected receive timeout.
    pub async fn wait_for_no_notification_matching(&self, member: &str, value: &str, duration: Duration) {
        let deadline = Instant::now() + duration;
        loop {
            assert!(
                self.take_matching(member, value).await.is_none(),
                "unexpected notification with {member}={value}"
            );
            let remaining = deadline.saturating_duration_since(Instant::now());
            let outcome = self.server.next_request(remaining).await;
            if matches!(outcome, Err(CallbackServerError::RequestTimeout(_))) {
                return;
            }
            let request = outcome.expect("notification server failed while checking absence");
            let body = request.json().expect("notification body is valid JSON");
            self.pending.lock().await.push(CapturedNotification {
                headers: request.headers,
                body,
            });
        }
    }

    /// Asserts that no notification for the given subscription arrives within `duration`.
    ///
    /// # Panics
    ///
    /// Panics when a notification for `subscription_id` arrives, or when the callback server
    /// fails for a reason other than the expected receive timeout.
    pub async fn wait_for_no_subscription_notification(&self, subscription_id: &str, duration: Duration) {
        self.wait_for_no_notification_matching("subscriptionId", subscription_id, duration).await;
    }

    /// Asserts that no notification for the given subscription arrives within `duration`.
    ///
    /// # Panics
    ///
    /// Panics when a notification for `subscription_id` arrives, or when the callback server
    /// fails for a reason other than the expected receive timeout.
    pub async fn wait_for_no_notification_for_subscription(&self, subscription_id: &str, duration: Duration) {
        self.wait_for_no_subscription_notification(subscription_id, duration).await;
    }

    /// Shuts the callback server down and releases its listening socket.
    ///
    /// # Panics
    ///
    /// Panics when the server task fails to terminate cleanly.
    pub async fn shutdown(self) {
        self.server.shutdown().await.expect("notification server shuts down");
    }

    async fn take_matching(&self, member: &str, value: &str) -> Option<CapturedNotification> {
        let mut pending = self.pending.lock().await;
        pending
            .iter()
            .position(|notification| notification.body[member].as_str() == Some(value))
            .map(|index| pending.remove(index))
    }
}
