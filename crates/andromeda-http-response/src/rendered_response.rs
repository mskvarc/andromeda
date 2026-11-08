//! A rendered response body and what the response carrying it says about it.
//!
//! Every body is written straight from the typed model into the response: an
//! entity, a temporal evolution or any other resource is serialised into its
//! body as it is, so no document of the resource is ever held beside it.

use crate::written_body::WrittenBody;
use andromeda_ngsi_ld_render::{
    body_response::ReadyBodyResponse,
    entity_response::ReadyEntityResponse,
    error::RenderError,
    response::ResponseHead,
    temporal::temporal_response::ReadyTemporalResponse,
};
use serde::Serialize;

/// A rendered response body with its head.
#[derive(Debug)]
pub struct RenderedResponse {
    /// The body, already written.
    pub(crate) body: WrittenBody,
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
        Ok(RenderedResponse { body, head })
    }

    /// Write a typed resource that is no entity (a Subscription, a
    /// Registration, a Snapshot, an `EntityMap`, a batch operation result or
    /// type information) into its body.
    ///
    /// # Errors
    ///
    /// Returns the [`RenderError`] of a resource that could not be written.
    pub fn resource<T: Serialize>(ready: ReadyBodyResponse<'_, '_, T>) -> Result<RenderedResponse, RenderError> {
        let mut body = WrittenBody::new();
        let head = ready.render_into(&mut body)?;
        Ok(RenderedResponse { body, head })
    }

    /// Write a typed temporal response into its body.
    ///
    /// # Errors
    ///
    /// Returns the [`RenderError`] of a temporal evolution that could not be
    /// written.
    pub fn temporal(ready: ReadyTemporalResponse<'_>) -> Result<RenderedResponse, RenderError> {
        let mut body = WrittenBody::new();
        let head = ready.render_into(&mut body)?;
        Ok(RenderedResponse { body, head })
    }
}
