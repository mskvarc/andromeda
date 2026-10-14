//! `POST /ngsi-ld/v1/notifications`.
//!
//! Two kinds of document arrive here, told apart by their `type`: a
//! `ContextSourceNotification` of a companion registration subscription, which
//! drives the reduced copies placed at context sources (clause 5.8.1.4), and a
//! `Notification` of a forwarded copy, which is re-assembled for the local
//! subscriber (clause 5.8.6). Either is decoded straight from the body's text.
//! The sender's `NGSILD-Tenant`, when present, is echoed back as clause 6.3.14
//! requires and never used for resolution: the tenant is the one that minted
//! the subscription identifier.

use crate::{
    error::{classify_forwarding, classify_not_a_notification, classify_resolution},
    state::NotificationReceiverState,
};
use andromeda_http_extraction::notification_body::NotificationBody;
use andromeda_http_headers::ngsild_tenant::NgsiLdTenant;
use andromeda_http_response::problem_response::ProblemResponse;
use andromeda_subscription_forwarding::inbound::Inbound;
use axum::{
    extract::{OriginalUri, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use headers::HeaderMapExt;

/// Receive one notification from a context source.
pub(crate) async fn receive(
    State(state): State<NotificationReceiverState>,
    OriginalUri(uri): OriginalUri,
    headers: HeaderMap,
    NotificationBody { text, context }: NotificationBody,
) -> Result<Response, ProblemResponse> {
    let received = Inbound::from_slice(&text).map_err(|error| classify_not_a_notification(error, &uri))?;
    // The resolved forwarding service carries a lease on its tenant's runtime;
    // holding it across handling keeps the runtime resident so eviction cannot
    // close its pools while the notification is processed.
    let resolved = state
        .resolver
        .resolve(received.subscription_id())
        .await
        .map_err(|error| classify_resolution(&error, &uri))?;
    // Both handlers hold a whole notification plus the reassembly state it is
    // merged into, which is far too large to keep inline in this handler's own
    // future; boxing keeps the request future small.
    match &received {
        Inbound::Notification(notification) => Box::pin(resolved.service.handle_notification(notification, &context)).await,
        Inbound::CsourceNotification(notification) => Box::pin(resolved.service.handle_csource_notification(notification, &context)).await,
    }
    .map_err(|error| classify_forwarding(&error, &uri))?;

    // Clause 6.3.14: the tenant header of the request is echoed on the response.
    let mut response = StatusCode::OK.into_response();
    if let Some(tenant) = headers.typed_get::<NgsiLdTenant>() {
        response.headers_mut().typed_insert(tenant);
    }
    Ok(response)
}
