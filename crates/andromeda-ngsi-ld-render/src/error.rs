//! What a rendering can fail at.
//!
//! Almost nothing. A rendering reads values that are already valid (the
//! resource was parsed on the way in and the representation options are typed),
//! so the only failure any of them can reach is `serde_json` refusing to
//! build a value. That is why this enum has one cause and names the rendering
//! it happened under rather than inventing categories nothing produces.

use strum::Display;
use thiserror::Error;

/// The rendering a serialisation failed under.
///
/// Carried so the message says which of the renderings could not be built;
/// none of them is recoverable, and the distinction exists for the operator
/// reading the log, not for the caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Display)]
pub enum RenderedShape {
    /// A clause 4.5.1 entity rendering.
    #[strum(serialize = "entity")]
    Entity,
    /// A clause 4.5.16 `GeoJSON` Feature.
    #[strum(serialize = "GeoJSON Feature")]
    GeoJsonFeature,
    /// A clause 4.5.16 `GeoJSON` `FeatureCollection`.
    #[strum(serialize = "GeoJSON FeatureCollection")]
    GeoJsonFeatureCollection,
    /// A clause 4.5.6 Temporal Evolution of an Entity.
    #[strum(serialize = "temporal entity")]
    TemporalEntity,
    /// A resource that is no entity: a Subscription, a Registration, a
    /// Snapshot, an `EntityMap`, a batch operation result or type information.
    #[strum(serialize = "resource")]
    Resource,
}

/// A rendering that could not be built.
#[derive(Debug, Error)]
pub enum RenderError {
    /// `serde_json` refused to build the value for a rendering.
    #[error("the {shape} rendering could not be serialised: {source}")]
    Serialization {
        /// Which rendering was being built.
        shape: RenderedShape,
        /// What `serde_json` reported.
        source: serde_json::Error,
    },
    /// A Temporal Evolution was asked for as `GeoJSON`, which clause 4.5.16
    /// defines for Entities only.
    #[error("a Temporal Evolution has no GeoJSON representation")]
    TemporalGeoJson,
}

impl RenderError {
    /// A serialisation failure under the given rendering.
    #[must_use]
    pub const fn serialization(shape: RenderedShape, source: serde_json::Error) -> RenderError {
        RenderError::Serialization { shape, source }
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::error::{RenderError, RenderedShape};

    /// A `serde_json::Error`, obtained the only way one can be: by asking
    /// `serde_json` to do something it refuses.
    fn a_serde_error() -> serde_json::Error {
        serde_json::from_str::<u8>("not a number").expect_err("the input is not a u8")
    }

    #[test]
    fn the_message_names_the_rendering_that_failed() {
        let error = RenderError::serialization(RenderedShape::GeoJsonFeatureCollection, a_serde_error());
        assert!(
            error
                .to_string()
                .starts_with("the GeoJSON FeatureCollection rendering could not be serialised:"),
            "unexpected message: {error}"
        );
    }

    #[test]
    fn every_shape_renders_a_distinct_name() {
        let shapes = [
            RenderedShape::Entity,
            RenderedShape::GeoJsonFeature,
            RenderedShape::GeoJsonFeatureCollection,
            RenderedShape::TemporalEntity,
            RenderedShape::Resource,
        ];
        let names: Vec<String> = shapes.iter().map(ToString::to_string).collect();
        let mut unique = names.clone();
        unique.sort();
        unique.dedup();
        pretty_assertions::assert_eq!(unique.len(), names.len(), "two shapes render the same name: {names:?}");
    }
}
