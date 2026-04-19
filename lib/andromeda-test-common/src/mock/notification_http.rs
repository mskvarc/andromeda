//! Axum-based notification receiver.
//!
//! Migrated from `tests/notification/helpers.rs:523-633`. Starts an axum
//! service on an ephemeral port, captures every POST body, and exposes
//! `wait_for_subscription_notification` / `wait_for_no_subscription_notification`
//! helpers for per-subscription assertions.

use axum::{Router, body::Bytes, http::HeaderMap as AxumHeaderMap, response::IntoResponse, routing::post};
use serde_json::Value;
use std::{sync::Arc, time::Duration};
use tokio::sync::{Mutex, Notify, oneshot};
use uuid::Uuid;

/// One captured inbound notification.
pub struct CapturedNotification {
    /// Request headers.
    pub headers: AxumHeaderMap,
    /// Parsed JSON body; `Value::Null` when the body was empty or not JSON.
    pub body: Value,
}

/// Notification receiver — binds to an ephemeral port and captures every
/// POST delivered to its endpoint.
pub struct NotificationReceiver {
    shutdown: Option<oneshot::Sender<()>>,
    /// Port the receiver is listening on.
    pub port: u16,
    endpoint_path: String,
    notifications: Arc<Mutex<Vec<CapturedNotification>>>,
    notify: Arc<Notify>,
}

impl NotificationReceiver {
    /// Bind, spawn, and return a ready receiver.
    pub async fn start() -> NotificationReceiver {
        let notifications = Arc::new(Mutex::new(Vec::<CapturedNotification>::new()));
        let notify = Arc::new(Notify::new());

        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let notification_store = notifications.clone();
        let signal = notify.clone();
        let endpoint_path = format!("/notify-{}", Uuid::new_v4());
        let route_path = endpoint_path.clone();

        let app = Router::new().route(
            &route_path,
            post(move |headers: AxumHeaderMap, body: Bytes| {
                let notification_store = notification_store.clone();
                let signal = signal.clone();
                async move {
                    let payload = serde_json::from_slice::<Value>(&body).unwrap_or(Value::Null);
                    notification_store.lock().await.push(CapturedNotification { headers, body: payload });
                    signal.notify_one();
                    axum::http::StatusCode::OK.into_response()
                }
            }),
        );

        let listener = match tokio::net::TcpListener::bind("0.0.0.0:0").await {
            Ok(listener) => listener,
            Err(error) => {
                eprintln!("NotificationReceiver failed to bind: {error}");
                std::process::exit(2);
            }
        };
        let port = match listener.local_addr() {
            Ok(address) => address.port(),
            Err(error) => {
                eprintln!("NotificationReceiver could not read local address: {error}");
                std::process::exit(2);
            }
        };

        tokio::spawn(async move {
            let _ = axum::serve(listener, app)
                .with_graceful_shutdown(async move {
                    let _ = shutdown_rx.await;
                })
                .await;
        });

        NotificationReceiver {
            shutdown: Some(shutdown_tx),
            port,
            endpoint_path,
            notifications,
            notify,
        }
    }

    /// External URL the broker dials. Uses `host.docker.internal` so a
    /// containerised broker can reach the host-bound test process.
    pub fn endpoint_url(&self) -> String {
        format!("http://host.docker.internal:{}{}", self.port, self.endpoint_path)
    }

    /// Drop every captured notification. Called between subscription
    /// scenarios in the same test.
    pub async fn clear_notifications(&self) {
        self.notifications.lock().await.clear();
    }

    /// Wait up to `timeout` for a notification whose `subscriptionId` matches
    /// `subscription_id`, then remove it from the capture buffer and return it.
    ///
    /// Exits with status 2 if the timeout is exceeded.
    pub async fn wait_for_subscription_notification(&self, subscription_id: &str, timeout: Duration) -> CapturedNotification {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            {
                let mut notifications = self.notifications.lock().await;
                if let Some(index) = notifications
                    .iter()
                    .position(|notification| notification.body["subscriptionId"].as_str() == Some(subscription_id))
                {
                    return notifications.remove(index);
                }
            }
            match tokio::time::timeout_at(deadline, self.notify.notified()).await {
                Ok(_) => {}
                Err(_) => {
                    eprintln!("timed out waiting for subscription {subscription_id} notification after {timeout:?}");
                    std::process::exit(2);
                }
            }
        }
    }

    /// Sleep `duration` and assert that no notification for `subscription_id`
    /// was captured in that window.
    pub async fn wait_for_no_subscription_notification(&self, subscription_id: &str, duration: Duration) {
        tokio::time::sleep(duration).await;
        let notifications = self.notifications.lock().await;
        let matching = notifications
            .iter()
            .filter(|notification| notification.body["subscriptionId"].as_str() == Some(subscription_id))
            .count();
        pretty_assertions::assert_eq!(matching, 0, "expected no notifications for subscription {subscription_id}, received {matching}");
    }

    /// Trigger graceful shutdown.
    pub fn stop(&mut self) {
        if let Some(shutdown_tx) = self.shutdown.take() {
            let _ = shutdown_tx.send(());
        }
    }
}

impl Drop for NotificationReceiver {
    fn drop(&mut self) {
        self.stop();
    }
}
