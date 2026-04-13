#![allow(clippy::expect_used, clippy::unwrap_used)]
use std::{collections::HashMap, sync::Arc};

use axum::response::IntoResponse;
use tokio::sync::RwLock;

pub struct ContextServer {
    pub port: u16,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
}

impl ContextServer {
    pub async fn start(routes: HashMap<String, String>) -> Self {
        let routes = Arc::new(RwLock::new(routes));

        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();

        let app = axum::Router::new().fallback(move |req: axum::extract::Request| {
            let store = routes.clone();
            async move {
                let path = req.uri().path().to_string();
                let routes = store.read().await;
                if let Some(body) = routes.get(&path) {
                    (
                        axum::http::StatusCode::OK,
                        [(axum::http::header::CONTENT_TYPE, "application/ld+json")],
                        body.clone(),
                    )
                        .into_response()
                } else {
                    axum::http::StatusCode::NOT_FOUND.into_response()
                }
            }
        });

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
        }
    }

    pub fn url(&self, path: &str) -> String {
        format!("http://host.docker.internal:{}{path}", self.port)
    }

    pub fn stop(&mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
    }
}
