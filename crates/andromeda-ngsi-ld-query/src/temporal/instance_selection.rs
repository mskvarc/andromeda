//! Which attribute instances a temporal query returns for a qualifying entity.

use crate::{dataset::DatasetSelector, temporal::last_n::LastNOrigin};
use iri_rs::IriBuf;
use typed_builder::TypedBuilder;

/// The instance-level filters of a temporal query.
///
/// Stage 2 of the two-stage statement is built from this: which attributes,
/// which `datasetId` partitions, and how many instances of each. All four apply
/// per entity, after [`EntitySelection`](crate::entity_selection::EntitySelection)
/// has decided which entities qualify.
#[derive(Debug, Clone, TypedBuilder)]
#[builder(field_defaults(default))]
pub struct InstanceSelection {
    /// Expanded attribute IRIs to return history for. Empty means every
    /// attribute.
    pub attr_filter: Vec<IriBuf>,
    /// Dataset instances to return (clause 4.5.5). Empty means every instance
    /// qualifies.
    pub dataset_id_filter: Vec<DatasetSelector>,
    /// Maximum number of instances per attribute.
    pub last_n: Option<u32>,
    /// Where the `lastN` limit came from.
    ///
    /// A system-imposed cap makes the response partial, which is a 206 and a
    /// `Content-Range` header; a limit the client asked for is neither.
    pub last_n_origin: LastNOrigin,
}
