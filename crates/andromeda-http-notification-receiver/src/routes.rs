//! Typed, state-complete receiver route inventory.

use crate::{
    handler,
    state::{NotificationReceiverResolver, NotificationReceiverState},
};
use axum::{Router, routing::post};
use std::sync::Arc;

/// The complete receiver route fragment.
pub struct NotificationReceiverRoutes {
    /// Router whose focused state has already been supplied.
    router: Router,
}

impl NotificationReceiverRoutes {
    /// Construct `POST /notifications` over a resolver.
    pub fn new(resolver: Arc<dyn NotificationReceiverResolver>) -> NotificationReceiverRoutes {
        let router = Router::new()
            .route("/notifications", post(handler::receive))
            .with_state(NotificationReceiverState { resolver });
        NotificationReceiverRoutes { router }
    }

    /// Consume the state-complete route fragment.
    pub fn into_router(self) -> Router {
        self.router
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        routes::NotificationReceiverRoutes,
        state::{LeasedForwarding, NotificationReceiverResolver, ReceiverResolutionError},
    };
    use andromeda_ngsi_ld_lexicon::uri::Uri;
    use axum::{
        body::{Body, to_bytes},
        http::{Request, StatusCode},
    };
    use futures::future::BoxFuture;
    use std::{error::Error, sync::Arc};
    use tower::ServiceExt;

    struct UnknownResolver;

    impl NotificationReceiverResolver for UnknownResolver {
        fn resolve<'a>(&'a self, subscription: &'a Uri) -> BoxFuture<'a, Result<LeasedForwarding, ReceiverResolutionError>> {
            Box::pin(async move {
                Err(ReceiverResolutionError::Unknown {
                    subscription: subscription.clone(), // clone: the error owns the identifier
                })
            })
        }
    }

    #[tokio::test]
    async fn a_notification_under_an_unknown_subscription_is_404_and_echoes_the_tenant() -> Result<(), Box<dyn Error>> {
        let body = serde_json::json!({
            "id": "urn:ngsi-ld:Notification:1",
            "type": "Notification",
            "subscriptionId": "urn:ngsi-ld:Subscription:unknown",
            "notifiedAt": "2024-01-01T00:00:00Z",
            "data": []
        });
        let body = body.to_string();
        let response = NotificationReceiverRoutes::new(Arc::new(UnknownResolver))
            .into_router()
            .oneshot(
                Request::post("/notifications")
                    .header("content-type", "application/json")
                    .header("content-length", body.len())
                    .header("NGSILD-Tenant", "peer")
                    .body(Body::from(body))?,
            )
            .await?;
        let status = response.status();
        let bytes = to_bytes(response.into_body(), usize::MAX).await?;
        pretty_assertions::assert_eq!(status, StatusCode::NOT_FOUND, "{}", String::from_utf8_lossy(&bytes));
        Ok(())
    }

    #[tokio::test]
    async fn a_document_that_is_not_a_notification_is_400() -> Result<(), Box<dyn Error>> {
        let body = r#"{"type": "Entity", "id": "urn:a"}"#;
        let response = NotificationReceiverRoutes::new(Arc::new(UnknownResolver))
            .into_router()
            .oneshot(
                Request::post("/notifications")
                    .header("content-type", "application/json")
                    .header("content-length", body.len())
                    .body(Body::from(body))?,
            )
            .await?;
        pretty_assertions::assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        Ok(())
    }
}
