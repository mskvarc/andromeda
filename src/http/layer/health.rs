//! Tower layer that injects [`Health`] as an axum extension.
//!
//! Applying this layer to a router makes the [`Health`] instance available to
//! handlers via `Extension<Health>`.

use crate::health::Health;
use axum::Extension;
use tower_layer::Layer;

impl<S> Layer<S> for Health {
    type Service = <Extension<Health> as Layer<S>>::Service;

    fn layer(&self, inner: S) -> Self::Service {
        Extension(self.clone()).layer(inner)
    }
}
