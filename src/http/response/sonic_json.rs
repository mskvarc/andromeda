//! `Json`-shaped response wrapper backed by `sonic-rs`.
//!
//! [`SonicJson`] mirrors [`axum::Json`] but serialises the inner payload
//! through `sonic_rs::to_vec`, which dispatches a SIMD-aware writer
//! (AVX2 / NEON / scalar) at runtime. The HTTP shape is identical:
//! `application/json` Content-Type, full body in a single `Bytes` chunk.
//!
//! Use this on hot response paths (batch operations, large collections)
//! to skip `serde_json`'s scalar-only writer.

use axum::{
    body::Body,
    http::{HeaderValue, StatusCode, header::CONTENT_TYPE},
    response::{IntoResponse, Response},
};
use serde::Serialize;

/// Response wrapper that serialises with `sonic-rs` instead of `serde_json`.
///
/// `T` must implement [`serde::Serialize`]. Conversion failures return
/// `500 Internal Server Error` with a plain-text body.
pub struct SonicJson<T>(pub T);

impl<T> IntoResponse for SonicJson<T>
where
    T: Serialize,
{
    fn into_response(self) -> Response {
        match sonic_rs::to_vec(&self.0) {
            Ok(buf) => {
                let mut response = Response::new(Body::from(buf));
                response.headers_mut().insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
                response
            }
            Err(err) => (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response(),
        }
    }
}

impl<T> From<T> for SonicJson<T> {
    fn from(inner: T) -> SonicJson<T> {
        SonicJson(inner)
    }
}
