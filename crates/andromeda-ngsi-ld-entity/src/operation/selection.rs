//! The named choices that steer a write: what it overwrites and which
//! instances of a multi-attribute it touches.

use andromeda_ngsi_ld_lexicon::identifier::Identifier;

/// Whether an append replaces an attribute instance that already exists
/// (clause 5.6.3, `noOverwrite` option).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OverwritePolicy {
    /// Existing instances are replaced by the incoming ones.
    Overwrite,
    /// Existing instances are kept and the incoming ones reported as not
    /// appended.
    Preserve,
}

/// Which instances of a multi-attribute an operation targets (clause 5.6.5).
///
/// One parameter rather than a `datasetId` plus a `deleteAll` flag, so
/// "delete everything, but only dataset A" cannot be expressed at all.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DatasetSelection<'a> {
    /// Every instance, regardless of `datasetId`.
    All,
    /// The single instance carrying this `datasetId`.
    Single(&'a Identifier),
    /// The default instance, the one with no `datasetId`.
    Default,
}
