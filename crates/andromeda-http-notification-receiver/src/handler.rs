//! `POST /ngsi-ld/v1/notifications`.
//!
//! Two kinds of document arrive here, told apart by their `type`: a
//! `ContextSourceNotification` of a companion registration subscription, which
//! drives the reduced copies placed at context sources (clause 5.8.1.4), and a
//! `Notification` of a forwarded copy, which is re-assembled for the local
//! subscriber (clause 5.8.6). The sender's `NGSILD-Tenant`, when present, is
//! echoed back as clause 6.3.14 requires and never used for resolution: the
//! tenant is the one that minted the subscription identifier.

use crate::{
    error::{bad_request_data, classify_forwarding, classify_resolution},
    state::NotificationReceiverState,
};
use andromeda_http_extraction::{context::ContextUrl, jsonld::JsonLd};
use andromeda_http_headers::ngsild_tenant::NgsiLdTenant;
use andromeda_http_response::problem_response::ProblemResponse;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::notification::{CsourceNotification, Notification};
use axum::{
    extract::{OriginalUri, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use headers::HeaderMapExt;
use serde_json::Value;

/// The two notification kinds the receiver accepts, discriminated by `type`.
#[derive(Debug)]
enum Received {
    /// A notification of a forwarded subscription copy (clause 5.3.1).
    Notification(Notification),
    /// A notification of a companion registration subscription (clause 5.3.2).
    ContextSourceNotification(CsourceNotification),
}

/// Why a document is not one of the notifications the receiver accepts.
#[derive(Debug, thiserror::Error)]
enum ReceivedError {
    /// `type` is absent or not a string.
    #[error("missing `type`")]
    MissingType,
    /// `type` names neither notification kind.
    #[error("unsupported type `{type_}`")]
    UnsupportedType {
        /// The offending value.
        type_: String,
    },
    /// The members do not form the named notification.
    #[error("{0}")]
    Shape(#[from] serde_json::Error),
}

// Each notification type carries its own constant `type` member, so an
// internally tagged enum would consume the discriminator the inner type needs.
impl TryFrom<Value> for Received {
    type Error = ReceivedError;

    fn try_from(document: Value) -> Result<Received, ReceivedError> {
        let type_ = document.get("type").and_then(Value::as_str).ok_or(ReceivedError::MissingType)?;
        match type_ {
            "Notification" => Ok(Received::Notification(serde_json::from_value(document)?)),
            "ContextSourceNotification" => Ok(Received::ContextSourceNotification(serde_json::from_value(document)?)),
            other => Err(ReceivedError::UnsupportedType { type_: other.to_owned() }),
        }
    }
}

impl Received {
    /// The subscription identifier the notification names.
    const fn subscription_id(&self) -> &Uri {
        match self {
            Received::Notification(notification) => &notification.subscription_id,
            Received::ContextSourceNotification(notification) => &notification.subscription_id,
        }
    }
}

/// Receive one notification from a context source.
pub(crate) async fn receive(
    State(state): State<NotificationReceiverState>,
    OriginalUri(uri): OriginalUri,
    headers: HeaderMap,
    context: ContextUrl,
    JsonLd {
        value: document,
        context: body_context,
    }: JsonLd<Value>,
) -> Result<Response, ProblemResponse> {
    let received = Received::try_from(document).map_err(|error| bad_request_data(format!("not a notification: {error}"), &uri))?;
    let context = context.merge(body_context);
    let service = state
        .resolver
        .resolve(received.subscription_id())
        .await
        .map_err(|error| classify_resolution(&error, &uri))?;
    match &received {
        Received::Notification(notification) => service.handle_notification(notification, &context).await,
        Received::ContextSourceNotification(notification) => service.handle_csource_notification(notification, &context).await,
    }
    .map_err(|error| classify_forwarding(&error, &uri))?;

    // Clause 6.3.14: the tenant header of the request is echoed on the response.
    let mut response = StatusCode::OK.into_response();
    if let Some(tenant) = headers.typed_get::<NgsiLdTenant>() {
        response.headers_mut().typed_insert(tenant);
    }
    Ok(response)
}
