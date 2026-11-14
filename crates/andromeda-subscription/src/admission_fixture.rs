//! The terms the tests of this crate admit a subscription's filters under.

use crate::filter_admission::SubscriptionFilterAdmission;
use andromeda_ngsi_ld_geometry::geometry_census::{GeometryCensusLimits, GeometryPositionLimit};
use andromeda_ngsi_ld_number::json_value::value_census::JsonValueLimit;
use andromeda_ngsi_ld_query::{
    filter::{complexity::FilterComplexityLimits, language_targeting::LanguageTargeting},
    geo::{admission::GeoQueryAdmission, near_distance_band::NearDistanceBand},
};
use std::num::NonZeroUsize;

/// Geometry limits no fixture reaches, with the distance band refused, the
/// broker's default `q` limits (1000 comparisons, 16 levels) and
/// `language_targeting`.
pub(crate) fn broker_defaults(language_targeting: LanguageTargeting) -> SubscriptionFilterAdmission {
    SubscriptionFilterAdmission {
        geo_q: GeoQueryAdmission {
            limits: GeometryCensusLimits {
                positions: GeometryPositionLimit::new(NonZeroUsize::MAX),
                values: JsonValueLimit::new(NonZeroUsize::MAX),
            },
            near_band: NearDistanceBand::Refused,
        },
        q_complexity: FilterComplexityLimits::new(
            NonZeroUsize::new(1000).expect("a positive comparison limit"),
            NonZeroUsize::new(16).expect("a positive nesting limit"),
        ),
        language_targeting,
    }
}
