//! A rendered response body and what the response carrying it says about it.
//!
//! A body reaches the response either as a JSON document the rendering
//! pipeline built, or already written: a typed entity response is serialised
//! straight into its body, so no document of the entity is ever held beside
//! it.

use crate::written_body::WrittenBody;
use andromeda_ngsi_ld_render::{
    entity_response::ReadyEntityResponse,
    error::RenderError,
    response::{RenderedOutput, ResponseHead},
};
use axum::{
    Json,
    response::{IntoResponse, Response},
};
use serde_json::Value;

/// A rendered response body with its head.
#[derive(Debug)]
pub struct RenderedResponse {
    /// The body.
    pub(crate) body: RenderedBody,
    /// What the response says about the body.
    pub(crate) head: ResponseHead,
}

impl RenderedResponse {
    /// Write a typed entity response into its body.
    ///
    /// # Errors
    ///
    /// Returns the [`RenderError`] of an entity that could not be written.
    pub fn entities(ready: ReadyEntityResponse<'_>) -> Result<RenderedResponse, RenderError> {
        let mut body = WrittenBody::new();
        let head = ready.render_into(&mut body)?;
        Ok(RenderedResponse {
            body: RenderedBody::Written(body),
            head,
        })
    }
}

// The document pipeline's output is one shape of a rendered response; the
// conversion moves it and cannot fail.
impl From<RenderedOutput> for RenderedResponse {
    fn from(rendered: RenderedOutput) -> RenderedResponse {
        RenderedResponse {
            body: RenderedBody::Document(rendered.value),
            head: rendered.head,
        }
    }
}

/// A response body in the form it was rendered in.
#[derive(Debug)]
pub(crate) enum RenderedBody {
    /// A JSON document, serialised when the response is sent.
    Document(Value),
    /// A body already written.
    Written(WrittenBody),
}

impl IntoResponse for RenderedBody {
    fn into_response(self) -> Response {
        match self {
            RenderedBody::Document(value) => Json(value).into_response(),
            RenderedBody::Written(body) => body.into_body().into_response(),
        }
    }
}
