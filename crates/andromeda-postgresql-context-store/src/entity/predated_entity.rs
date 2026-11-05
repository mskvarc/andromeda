//! One Entity a statement did not write, because it holds an instant later
//! than the write's.

use andromeda_ngsi_ld_lexicon::{identifier::Identifier, time::system_instant::SystemInstant};

/// An Entity whose write one statement refused, because the Entity holds an
/// instant later than the one the write was stamped at (clause 4.8).
#[derive(Debug)]
pub struct PredatedEntity {
    /// The Entity the write named.
    pub id: Identifier,
    /// The latest instant the Entity holds, which the write is stamped after
    /// when it is attempted again.
    pub latest: SystemInstant,
}
