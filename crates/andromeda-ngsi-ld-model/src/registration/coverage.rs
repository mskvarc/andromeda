//! What a registered Context Source answers for, in space, time and selection (ETSI GS CIM 009 clauses 5.2.9 and 5.2.10).

use crate::{registration::info::RegistrationInfo, time_interval::TimeInterval};
use andromeda_ngsi_ld_geometry::geometry::Geometry;
use andromeda_ngsi_ld_lexicon::scope::path::ScopePath;
use andromeda_ngsi_ld_query::dataset::DatasetSelector;
use serde::{Deserialize, Serialize};
use typed_builder::TypedBuilder;

/// What a context source covers: the entity and attribute selectors it answers
/// for, and the space and time its data spans (ETSI GS CIM 009 clauses 5.2.9
/// and 5.2.10).
#[derive(Debug, Clone, Serialize, Deserialize, TypedBuilder)]
#[serde(rename_all = "camelCase")]
pub struct RegistrationCoverage {
    /// Information blocks describing what this source provides.
    pub information: Vec<RegistrationInfo>,
    /// The `datasetId`s of the attribute instances the source can provide
    /// (clause 4.5.5); `@none` names the default instance.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(default = Default::default())]
    pub dataset_id: Option<Vec<DatasetSelector>>,
    /// The clause 4.18 scopes the source has entities in. A registration names
    /// places, so the NGSI-LD Null a deleted entity scope is written as is not
    /// one of its values.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(default = Default::default())]
    pub scope: Option<Vec<ScopePath>>,
    /// Location geometry.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(default = Default::default())]
    pub location: Option<Geometry>,
    /// Observation space geometry.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(default = Default::default())]
    pub observation_space: Option<Geometry>,
    /// Operation space geometry.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(default = Default::default())]
    pub operation_space: Option<Geometry>,
    /// Interval the source's observations span (clause 5.2.11).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(default = Default::default())]
    pub observation_interval: Option<TimeInterval>,
    /// Interval the source's management data spans (clause 5.2.11).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[builder(default = Default::default())]
    pub management_interval: Option<TimeInterval>,
}
