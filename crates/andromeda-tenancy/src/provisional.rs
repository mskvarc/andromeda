//! How a provisional tenant settles: established once its database holds
//! information, retired once it has stayed empty past its time to live.

/// The outcome of settling a provisional tenant against its database.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Establishment {
    /// The tenant exists: its database holds information, so the registry now
    /// records it as established, or it was established already.
    Established,
    /// The tenant does not exist: it is provisional and its database holds no
    /// information, or the registry holds no row for it.
    Empty,
}

/// The outcome of offering a provisional tenant for retirement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Retirement {
    /// The tenant's database is dropped and its registry row removed.
    Retired,
    /// The tenant's database holds information, so it was established instead.
    Established,
    /// The tenant stays as it is: touched within its time to live, connected to
    /// by some session, no longer provisional, or no longer registered.
    Kept,
}
