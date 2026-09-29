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

use andromeda_http_extraction::context_target::ExistingTargetedTenantState;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use axum::{
    extract::OriginalUri,
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
    error::{SubscriptionResultExt, public_problem},
    state::SubscriptionTenantState,
};
use andromeda_delivery::{
    endpoint::transport::{EndpointTransport, parse_endpoint_transport},
    stream::{
        registry::{ConnectionId, ConnectionRegistry},
        sse::SseNotificationEvent,
    },
};
use andromeda_http_extraction::typed_path::TypedPath;
use andromeda_http_response::problem_response::ProblemResponse;

/// RAII guard that unregisters an SSE connection when the stream ends.
struct SseConnectionGuard {
    subscription_id: Identifier,
    connection_id: ConnectionId,
    registry: Arc<ConnectionRegistry<SseNotificationEvent>>,
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
    rx: mpsc::Receiver<SseNotificationEvent>,
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
                let event = Event::default().id(&notification.event_id).event("notification").data(notification.data);
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
pub async fn stream(
    ExistingTargetedTenantState { services: state, target, .. }: SubscriptionTenantState,
    OriginalUri(uri): OriginalUri,
    TypedPath(subscription_id): TypedPath<Identifier>,
) -> Result<Response, ProblemResponse> {
    // Validate subscription exists.
    let subscription = state.subscriptions.get(&target, &subscription_id).await.with_subscription_instance(&uri)?;

    // Validate subscription uses SSE transport via the canonical parser
    // (handles case-insensitive URI schemes like SSE://).
    let is_sse = parse_endpoint_transport(
        subscription.notification.endpoint.uri.url(),
        subscription.notification.endpoint.notifier_info.as_deref(),
    )
    .is_ok_and(|t| matches!(t, EndpointTransport::Sse));

    if !is_sse {
        return Err(public_problem(
            "Subscription endpoint is not an SSE transport. Only subscriptions with sse:// endpoint URI support streaming.",
            &uri,
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
