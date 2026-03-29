use std::{sync::Arc, time::Duration};

use axum::{body::Bytes, http::HeaderMap, response::IntoResponse};
use tokio::sync::Mutex;

pub struct CapturedNotification {
    pub headers: HeaderMap,
    pub body: serde_json::Value,
}

pub struct NotificationReceiver {
    pub port: u16,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
    notifications: Arc<Mutex<Vec<CapturedNotification>>>,
    notify: Arc<tokio::sync::Notify>,
}

impl NotificationReceiver {
    pub async fn start() -> Self {
        let notifications: Arc<Mutex<Vec<CapturedNotification>>> = Arc::new(Mutex::new(Vec::new()));
        let notify = Arc::new(tokio::sync::Notify::new());

        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();

        let notifs = notifications.clone();
        let ntf = notify.clone();

        let app = axum::Router::new().route(
            "/notify",
            axum::routing::post(move |headers: HeaderMap, body: Bytes| {
                let notifs = notifs.clone();
                let ntf = ntf.clone();
                async move {
                    let json: serde_json::Value = serde_json::from_slice(&body).unwrap_or(serde_json::Value::Null);
                    notifs.lock().await.push(CapturedNotification { headers, body: json });
                    ntf.notify_one();
                    axum::http::StatusCode::OK.into_response()
                }
            }),
        );

        let listener = tokio::net::TcpListener::bind("0.0.0.0:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = shutdown_rx.await;
                })
                .await
                .unwrap();
        });

        Self {
            port,
            shutdown: Some(shutdown_tx),
            notifications,
            notify,
        }
    }

    pub fn endpoint_url(&self) -> String {
        format!("http://host.docker.internal:{}/notify", self.port)
    }

    pub async fn wait_for_notification(&self, timeout: Duration) -> CapturedNotification {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            {
                let mut notifs = self.notifications.lock().await;
                if !notifs.is_empty() {
                    return notifs.remove(0);
                }
            }
            if tokio::time::timeout_at(deadline, self.notify.notified()).await.is_err() {
                panic!("Timed out waiting for notification after {:?}", timeout);
            }
        }
    }

    pub async fn wait_for_no_notification(&self, duration: Duration) {
        tokio::time::sleep(duration).await;
        let notifs = self.notifications.lock().await;
        assert!(notifs.is_empty(), "Expected no notifications, but received {}", notifs.len());
    }

    pub fn stop(&mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
    }
}

impl Drop for NotificationReceiver {
    fn drop(&mut self) {
        self.stop();
    }
}
