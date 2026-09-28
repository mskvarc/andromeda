//! Which attribute instances a temporal query returns for a qualifying entity.

use crate::{dataset::DatasetSelector, temporal::instance_page::InstancePage};
use iri_rs::IriBuf;
use typed_builder::TypedBuilder;

/// The instance-level filters of a temporal query.
///
/// Stage 2 of the two-stage statement is built from this: which attributes,
/// which `datasetId` partitions, and which instances of each. All three apply
/// per entity, after [`EntitySelection`](crate::entity_selection::EntitySelection)
/// has decided which entities qualify.
#[derive(Debug, Clone, TypedBuilder)]
#[builder(field_defaults(default = Default::default()))]
pub struct InstanceSelection {
    /// Expanded attribute IRIs to return history for. Empty means every
    /// attribute.
    pub attr_filter: Vec<IriBuf>,
    /// Dataset instances to return (clause 4.5.5). Empty means every instance
    /// qualifies.
    pub dataset_id_filter: Vec<DatasetSelector>,
    /// Which instances of every attribute stream are kept, and whether the
    /// ones left out make the response partial (clause 6.3.10).
    pub page: InstancePage,
}
