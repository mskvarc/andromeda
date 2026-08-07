//! The named choices that steer a mutation: what it overwrites and which
//! instances of a multi-attribute it touches.

use urn_rs::Urn;

/// Whether an append replaces an attribute instance that already exists
/// (NGSI-LD clause 5.6.3, `noOverwrite` option).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverwritePolicy {
    /// Existing instances are replaced by the incoming ones.
    Overwrite,
    /// Existing instances are kept and the incoming ones reported as not appended.
    Preserve,
}

/// Which instances of a multi-attribute an operation targets
/// (NGSI-LD clause 5.6.5).
///
/// One parameter rather than a `datasetId` plus a `deleteAll` flag, so
/// "delete everything, but only dataset A" cannot be expressed at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DatasetSelection<'a> {
    /// Every instance, regardless of `datasetId`.
    All,
    /// The single instance carrying this `datasetId`.
    Single(&'a Urn),
    /// The default instance, the one with no `datasetId`.
    Default,
}
