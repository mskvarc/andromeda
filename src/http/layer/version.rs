//! Tower layer that injects [`Version`] as an axum extension.
//!
//! Applying this layer to a router makes the [`Version`] instance available to
//! handlers via `Extension<Version>`.

use crate::version::service::Version;
use axum::Extension;
use tower_layer::Layer;

impl<S> Layer<S> for Version {
    type Service = <Extension<Version> as Layer<S>>::Service;

    fn layer(&self, inner: S) -> Self::Service {
        Extension(self.clone()).layer(inner)
    }
}
