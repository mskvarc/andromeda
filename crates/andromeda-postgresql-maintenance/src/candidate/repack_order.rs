//! The order a repack copies a heap's rows in.

use crate::candidate::index_target::IndexTarget;

/// The physical order a repack writes a heap's rows in.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RepackOrder {
    /// The order the rows are read in, as `VACUUM FULL` writes them: the
    /// cheapest copy, for a heap no read walks in key order.
    Physical,
    /// The order of one B-tree's key, as `CLUSTER` writes them, so the rows a
    /// read fetches by a prefix of that key share pages.
    ClusteredBy(IndexTarget),
}
