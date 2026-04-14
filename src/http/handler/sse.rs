//! SSE stream handler for subscription notifications.
//!
//! Provides a `GET /subscriptions/{subscriptionId}/stream` endpoint that
//! opens a persistent Server-Sent Events connection for receiving real-time
//! notifications from an SSE-transport subscription.

use std::{
    convert::Infallible,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
    time::Duration,
};

use axum::{
    extract::{OriginalUri, Path},
    response::{
        IntoResponse,
        Response,
        sse::{Event, KeepAlive, Sse},
    },
};
use futures::Stream;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::{
    api::tenant::TenantState,
    core::error::{AndromedaError, ResultInstanceExt},
    http::error::problem::NgsiLdError,
    transport::{
        endpoint::{EndpointTransport, parse_endpoint_transport},
        sse::{ConnectionId, SseConnectionRegistry},
    },
};

/// RAII guard that unregisters an SSE connection when the stream ends.
struct SseConnectionGuard {
    subscription_id: String,
    connection_id: ConnectionId,
    registry: Arc<SseConnectionRegistry>,
}

impl Drop for SseConnectionGuard {
    fn drop(&mut self) {
        self.registry.unregister(&self.subscription_id, self.connection_id);
    }
}

/// SSE event stream that combines notification reception with cancellation.
///
/// Yields SSE events from the mpsc receiver until either the channel closes
/// or the cancellation token fires. Holds the connection guard to ensure
/// cleanup when the stream is dropped.
struct SseEventStream {
    rx: mpsc::Receiver<crate::transport::sse::SseNotificationEvent>,
    cancellation_token: CancellationToken,
    _guard: SseConnectionGuard,
}

impl Stream for SseEventStream {
    type Item = Result<Event, Infallible>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        // Check cancellation first.
        if self.cancellation_token.is_cancelled() {
            return Poll::Ready(None);
        }

        match self.rx.poll_recv(cx) {
            Poll::Ready(Some(notification)) => {
                let event = Event::default().id(notification.event_id).event("notification").data(notification.data);
                Poll::Ready(Some(Ok(event)))
            }
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }
}

/// `GET /ngsi-ld/v1/subscriptions/{subscriptionId}/stream` -- Open an SSE notification stream.
///
/// Validates that the subscription exists and uses SSE transport, then opens a
/// persistent connection that streams notification events as they are dispatched
/// by the notification worker.
pub async fn stream(TenantState(state): TenantState, OriginalUri(uri): OriginalUri, Path(subscription_id): Path<String>) -> Result<Response, NgsiLdError> {
    // Validate subscription exists.
    let subscription = state.subscriptions.get(&subscription_id).await.with_instance(uri.clone())?;

    // Validate subscription uses SSE transport via the canonical parser
    // (handles case-insensitive URI schemes like SSE://).
    let is_sse = parse_endpoint_transport(
        &subscription.notification.endpoint.uri,
        subscription.notification.endpoint.notifier_info.as_deref(),
    )
    .map_or(false, |t| matches!(t, EndpointTransport::Sse));

    if !is_sse {
        return Err(NgsiLdError::from(
            AndromedaError::subscription_validation(
                "Subscription endpoint is not an SSE transport. Only subscriptions with sse:// endpoint URI support streaming.",
            )
            .with_instance(uri.clone()),
        ));
    }

    // Get SSE registry and config.
    let sse_registry = Arc::clone(&state.sse_registry);
    let keep_alive_secs = state.sse_keep_alive_interval_secs;
    let buffer_size = state.sse_channel_buffer_size;

    // Register connection.
    let (connection_id, rx, cancellation_token) = sse_registry.register(&subscription_id, buffer_size);

    // Build the event stream with cleanup guard.
    let event_stream = SseEventStream {
        rx,
        cancellation_token,
        _guard: SseConnectionGuard {
            subscription_id,
            connection_id,
            registry: sse_registry,
        },
    };

    // Return SSE response with keep-alive.
    let sse = Sse::new(event_stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(keep_alive_secs)));

    Ok(sse.into_response())
}
