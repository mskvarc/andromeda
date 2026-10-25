//! The budget one written entity is admitted under (ETSI GS CIM 009 v1.9.1
//! clause 6.3.2 answers a request whose input "was too large" with 413
//! `RequestEntityTooLarge`), and what a payload over it is refused with.
//!
//! The budget is taken per request: it bounds what one write makes the broker
//! hold, not what an entity grows to over many writes. It is counted on the
//! written document the typed expansion produced, by the layer that admits
//! the write. The same walk holds each attribute to the nesting its stored
//! expansion can be read back at, which is refused on its own terms (see
//! [`crate::attribute_nesting`]).

use crate::attribute_nesting::AttributeNestingLimit;
use andromeda_ngsi_ld_geometry::geometry_census::{GeometryCensusLimits, GeometryPositionLimit};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_number::json_value::value_census::JsonValueLimit;
use derive_more::Display;
use thiserror::Error;

/// The budget a written entity payload is admitted under.
///
/// Every replica of a deployment runs the same budget: a load balancer may hand
/// any request to any of them, and an entity one admitted is read and deleted
/// by the others.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EntityPayloadBudget {
    /// The most JSON values the payload may hold outside its geometries.
    pub json_values: JsonValueLimit,
    /// The most positions any one geometry of the payload may hold.
    pub geometry_positions: GeometryPositionLimit,
    /// The deepest nesting any one attribute of the payload may have.
    pub attribute_nesting: AttributeNestingLimit,
}

/// What a payload's census found over its budget.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum PayloadBudgetExceeded {
    /// The payload holds more JSON values outside its geometries' positions
    /// than the entity limit admits.
    #[error(
        "the entity holds more than the {limit} JSON values this broker accepts in one entity, geometry positions and JSON literals aside; send its values over several entities or requests"
    )]
    EntityJsonValues {
        /// The limit the payload exceeds.
        limit: JsonValueLimit,
    },
    /// A geometry of the payload holds more positions than the limit admits.
    #[error("{owner} holds more than the {limit} positions this broker accepts in one geometry; send a simplified geometry")]
    GeometryPositions {
        /// The attribute whose geometry exceeds the limit.
        owner: GeometryOwner,
        /// The limit the geometry exceeds.
        limit: GeometryPositionLimit,
    },
}

/// The attribute a refused geometry belongs to.
#[derive(Clone, Debug, PartialEq, Eq, Display)]
pub enum GeometryOwner {
    /// A geometry of the attribute of this name.
    #[display("the geometry of the attribute '{_0}'")]
    Attribute(AttributeName),
    /// A geometry of an attribute whose name is no attribute name, which the
    /// attribute admission refuses on its own account.
    #[display("a geometry")]
    Unnamed,
}

impl EntityPayloadBudget {
    /// The limits a geometry standing on its own is admitted under: as many
    /// positions as one geometry of an entity, and besides them as many
    /// other values as one entity.
    #[must_use]
    pub const fn geometry_census_limits(self) -> GeometryCensusLimits {
        GeometryCensusLimits {
            positions: self.geometry_positions,
            values: self.json_values,
        }
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::payload_budget::{GeometryOwner, PayloadBudgetExceeded};
    use andromeda_ngsi_ld_geometry::geometry_census::GeometryPositionLimit;
    use std::num::NonZeroUsize;

    /// The refusal names the attribute whose geometry is over the limit and
    /// the limit, and says what to send instead.
    #[test]
    fn a_geometry_refusal_names_its_attribute_and_the_limit() {
        let refusal = PayloadBudgetExceeded::GeometryPositions {
            owner: GeometryOwner::Attribute("https://uri.etsi.org/ngsi-ld/location".parse().expect("an attribute name")),
            limit: GeometryPositionLimit::new(NonZeroUsize::new(100_000).expect("a positive limit")),
        };
        let text = refusal.to_string();
        assert!(text.contains("'https://uri.etsi.org/ngsi-ld/location'"), "{text}");
        assert!(text.contains("100000"), "{text}");
        assert!(text.contains("send a simplified geometry"), "{text}");
    }
}
