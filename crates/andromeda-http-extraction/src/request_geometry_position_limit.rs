//! The most positions one geometry of a request payload may hold.

use andromeda_ngsi_ld_geometry::geometry_census::GeometryPositionLimit;
use andromeda_ngsi_ld_model::request_body::RequestBody;
use andromeda_ngsi_ld_representation::compact_node::context_reading::ContextReading;

/// The maximum number of positions one geometry of a request payload may hold,
/// carried as a request extension.
///
/// The value limit of [`RequestJsonValueLimit`](crate::request_json_value_limit::RequestJsonValueLimit)
/// bounds the payload, not what a geometry costs once it is read: a geometry is
/// recorded ordinate by ordinate, and a payload may hold millions of them. The
/// HTTP stack installs this limit beside the value limit on every NGSI-LD
/// request, and the decoding of a body stops recording a geometry at the first
/// ordinate no geometry within the limit can hold, so a geometry that will be
/// refused (clause 6.3.2) costs what the bound costs whatever its size. A
/// request carrying none -- a router assembled without the stack, as in a unit
/// test -- records every geometry whole.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RequestGeometryPositionLimit(GeometryPositionLimit);

impl RequestGeometryPositionLimit {
    /// Construct a request geometry position limit.
    #[must_use]
    pub const fn new(limit: GeometryPositionLimit) -> RequestGeometryPositionLimit {
        RequestGeometryPositionLimit(limit)
    }

    /// The number of positions a geometry may hold.
    #[must_use]
    pub const fn limit(self) -> GeometryPositionLimit {
        self.0
    }
}

/// Read the body `text` as a `T` through `reading`, its geometries recorded
/// within `limit` when the stack published one.
///
/// # Errors
///
/// Returns the reader's failure, positioned in the text.
pub fn read_within<T: RequestBody>(limit: Option<RequestGeometryPositionLimit>, text: &[u8], reading: ContextReading<'_>) -> Result<T, serde_json::Error> {
    match limit {
        Some(limit) => T::from_bounded_request_text(text, reading, limit.limit()),
        None => T::from_request_text(text, reading),
    }
}

#[cfg(test)]
mod tests {
    use crate::request_geometry_position_limit::RequestGeometryPositionLimit;
    use andromeda_ngsi_ld_geometry::geometry_census::GeometryPositionLimit;
    use std::num::NonZeroUsize;

    #[test]
    fn a_limit_carries_the_position_count_it_was_built_with() {
        let limit = GeometryPositionLimit::new(NonZeroUsize::MIN.saturating_add(999_999));
        pretty_assertions::assert_eq!(RequestGeometryPositionLimit::new(limit).limit().get(), 1_000_000);
    }
}
