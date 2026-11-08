//! How much of a request payload is recorded before the rest is skipped.

use andromeda_ngsi_ld_geometry::geometry_census::GeometryPositionLimit;
use andromeda_ngsi_ld_model::request_body::{DecodeBounds, RequestBody};
use andromeda_ngsi_ld_number::json_value::value_census::JsonValueLimit;
use andromeda_ngsi_ld_representation::compact_node::context_reading::ContextReading;

/// The bounds a request payload is recorded within, carried as a request
/// extension.
///
/// The value limit of [`RequestJsonValueLimit`](crate::request_json_value_limit::RequestJsonValueLimit)
/// bounds the payload, not what it costs once it is read: a geometry is
/// recorded ordinate by ordinate, a language map entry by entry, and a payload
/// may hold millions of either. The HTTP stack installs these bounds beside the
/// value limit on every NGSI-LD request, and the decoding of a body stops
/// recording a geometry at the first ordinate no geometry within the bound can
/// hold, and an entity at the first value past the entity value limit, so a
/// payload that will be refused (clause 6.3.2) costs what the bounds cost
/// whatever its size. A request carrying none (a router assembled without the
/// stack, as in a unit test) records every payload whole.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RequestDecodeBounds(DecodeBounds);

impl RequestDecodeBounds {
    /// Construct the bounds a request payload is recorded within: a geometry
    /// of at most `geometry_positions` positions and an entity of at most
    /// `entity_values` values.
    #[must_use]
    pub const fn new(geometry_positions: GeometryPositionLimit, entity_values: JsonValueLimit) -> RequestDecodeBounds {
        RequestDecodeBounds(DecodeBounds::new(geometry_positions, entity_values))
    }

    /// The bounds.
    #[must_use]
    pub const fn bounds(self) -> DecodeBounds {
        self.0
    }
}

/// Read the body `text` as a `T` through `reading`, recorded within `bounds`
/// when the stack published them.
///
/// # Errors
///
/// Returns the reader's failure, positioned in the text.
pub fn read_within<T: RequestBody>(bounds: Option<RequestDecodeBounds>, text: &[u8], reading: ContextReading<'_>) -> Result<T, serde_json::Error> {
    match bounds {
        Some(bounds) => T::from_bounded_request_text(text, reading, bounds.bounds()),
        None => T::from_request_text(text, reading),
    }
}

#[cfg(test)]
mod tests {
    use crate::request_decode_bounds::RequestDecodeBounds;
    use andromeda_ngsi_ld_geometry::geometry_census::GeometryPositionLimit;
    use andromeda_ngsi_ld_number::json_value::value_census::JsonValueLimit;
    use std::num::NonZeroUsize;

    #[test]
    fn the_bounds_carry_the_limits_they_were_built_with() {
        let carried = RequestDecodeBounds::new(
            GeometryPositionLimit::new(NonZeroUsize::MIN.saturating_add(999_999)),
            JsonValueLimit::new(NonZeroUsize::MIN.saturating_add(499_999)),
        )
        .bounds();
        pretty_assertions::assert_eq!(carried.geometry_positions().get(), 1_000_000);
        pretty_assertions::assert_eq!(carried.entity_values().get(), 500_000);
    }
}
