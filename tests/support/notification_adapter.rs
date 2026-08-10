//! JSON notification adapter over the shared raw callback server.

use andromeda_test_mocks::{callback_server::CallbackServerError, notification_server::NotificationServer, server_config::MockServerConfig};
use axum::http::HeaderMap;
use serde_json::Value;
use std::time::Duration;
use tokio::sync::Mutex;

pub struct CapturedNotification {
    pub headers: HeaderMap,
    pub body: Value,
}

pub struct NotificationAdapter {
    server: NotificationServer,
    pending: Mutex<Vec<CapturedNotification>>,
}

impl NotificationAdapter {
    pub async fn start() -> NotificationAdapter {
        let configuration = MockServerConfig::from_environment().expect("valid notification server configuration");
        let server = NotificationServer::start(configuration).await.expect("notification server starts");
        NotificationAdapter {
            server,
            pending: Mutex::new(Vec::new()),
        }
    }

    pub fn endpoint_url(&self) -> String {
        self.server.endpoint_url().expect("notification endpoint resolves")
    }

    pub async fn clear_notifications(&self) {
        self.pending.lock().await.clear();
        let _drained = self.server.drain_requests().await;
    }

    pub async fn wait_for_subscription_notification(&self, subscription_id: &str, timeout: Duration) -> CapturedNotification {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            if let Some(notification) = self.take_matching(subscription_id).await {
                return notification;
            }
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            let request = self.server.next_request(remaining).await.expect("notification arrives before timeout");
            let body = request.json().expect("notification body is valid JSON");
            self.pending.lock().await.push(CapturedNotification {
                headers: request.headers,
                body,
            });
        }
    }

    pub async fn wait_for_no_subscription_notification(&self, subscription_id: &str, duration: Duration) {
        let deadline = tokio::time::Instant::now() + duration;
        loop {
            assert!(
                self.take_matching(subscription_id).await.is_none(),
                "unexpected notification for subscription {subscription_id}"
            );
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            match self.server.next_request(remaining).await {
                Ok(request) => {
                    let body = request.json().expect("notification body is valid JSON");
                    self.pending.lock().await.push(CapturedNotification {
                        headers: request.headers,
                        body,
                    });
                }
                Err(CallbackServerError::RequestTimeout(_)) => return,
                Err(error) => unreachable!("notification server failed while checking absence: {error}"),
            }
        }
    }

    pub async fn wait_for_no_notification_for_subscription(&self, subscription_id: &str, duration: Duration) {
        self.wait_for_no_subscription_notification(subscription_id, duration).await;
    }

    pub async fn shutdown(self) {
        self.server.shutdown().await.expect("notification server shuts down");
    }

    async fn take_matching(&self, subscription_id: &str) -> Option<CapturedNotification> {
        let mut pending = self.pending.lock().await;
        pending
            .iter()
            .position(|notification| notification.body["subscriptionId"].as_str() == Some(subscription_id))
            .map(|index| pending.remove(index))
    }
}
