//! The [`RegistrationService`] policy unit tests run against.

use crate::{
    RegistrationChangeSink,
    RegistrationService,
    entity_probe::EntityPresence,
    fixed_probe::FixedProbe,
    registration_census::RegistrationCensus,
    store::RegistrationStore,
};
use andromeda_jsonld::engine::processor::JsonLdEngine;
use andromeda_ngsi_ld_geometry::geometry_census::{GeometryCensusLimits, GeometryPositionLimit};
use andromeda_ngsi_ld_number::json_value::value_census::JsonValueLimit;
use andromeda_ngsi_ld_query::geo::{admission::GeoQueryAdmission, near_distance_band::NearDistanceBand};
use std::{num::NonZeroUsize, sync::Arc};

/// An engine with remote context fetching disabled, so a remote context can
/// only fail to load.
pub(crate) fn offline_engine() -> JsonLdEngine {
    JsonLdEngine::builder().allow_remote(false).build().expect("engine builds")
}

/// A service over `store` reading contexts through `engine`, probing no
/// entity and publishing its change events to `sink`, with geometry limits no
/// fixture reaches, counting none of the store's registrations.
pub(crate) fn service_over(store: Arc<dyn RegistrationStore>, engine: JsonLdEngine, sink: Option<Arc<dyn RegistrationChangeSink>>) -> RegistrationService {
    service_counting(store, RegistrationCensus::new(0), engine, sink)
}

/// A service like [`service_over`] that counts its registrations in `census`.
pub(crate) fn service_counting(
    store: Arc<dyn RegistrationStore>,
    census: RegistrationCensus,
    engine: JsonLdEngine,
    sink: Option<Arc<dyn RegistrationChangeSink>>,
) -> RegistrationService {
    RegistrationService::new(
        store,
        census,
        Arc::new(FixedProbe::new(EntityPresence::Absent, EntityPresence::Absent)),
        engine,
        sink,
        GeoQueryAdmission::new(
            GeometryCensusLimits {
                positions: GeometryPositionLimit::new(NonZeroUsize::MIN.saturating_add(999_999)),
                values: JsonValueLimit::new(NonZeroUsize::MIN.saturating_add(999_999)),
            },
            NearDistanceBand::Refused,
        ),
    )
}
