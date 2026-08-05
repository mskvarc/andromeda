//! Result-window parameters shared by every compiled query variant.

/// Whether a query reports the total number of matches (clause 6.3.13).
///
/// The distinction is not cosmetic: emitting the count means running a second
/// statement over the same predicate, so this decides whether that statement is
/// built at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CountPolicy {
    /// Build and run the count statement, and emit `NGSILD-Results-Count`.
    Emit,
    /// Skip the count statement entirely.
    Omit,
}

/// Pagination parameters shared by every compiled query variant.
///
/// Consolidates the `limit`, `offset`, and `count` trio that used to be
/// repeated on `CompiledEntityQuery`, `CompiledTemporalQuery`, and
/// `CompiledCsrQuery`. Construct via `Pagination { ... }` or `Pagination::DEFAULT`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pagination {
    /// Maximum number of results to return (NGSI-LD 5.5.6).
    pub limit: u32,
    /// Number of results to skip for pagination (NGSI-LD 5.5.6).
    pub offset: u32,
    /// Whether to emit a total-count header (NGSI-LD 6.3.13).
    pub count: CountPolicy,
}

impl Pagination {
    /// Default pagination: limit 20, offset 0, count disabled.
    pub const DEFAULT: Pagination = Pagination {
        limit: 20,
        offset: 0,
        count: CountPolicy::Omit,
    };
}

// Hand-written because `#[derive(Default)]` would yield a limit of zero, which
// returns nothing. Clause 5.5.6 makes the default page size a broker decision,
// so the default is a value, not the absence of one.
impl Default for Pagination {
    fn default() -> Pagination {
        Pagination::DEFAULT
    }
}
