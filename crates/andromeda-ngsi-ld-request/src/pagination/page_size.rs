//! How many elements a page of one collection holds (clause 6.3.10 `limit`).

/// The page size a collection answers with when the request names none, and
/// the most it answers with when the request names one.
///
/// Clause 5.5.6 makes both a broker decision, and they differ per collection:
/// a query over entities is capped so one request cannot ask for an unbounded
/// result set, while the broker's own administrative collections are small
/// enough to be read whole.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageSizeBounds {
    /// The page size of a request that names no `limit`.
    pub default: u32,
    /// The largest page size a request may ask for.
    pub cap: PageSizeCap,
}

/// Whether a collection bounds the page size a request may ask for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageSizeCap {
    /// A requested `limit` above this value is lowered to it.
    Capped(u32),
    /// A requested `limit` is honoured as asked.
    Uncapped,
}

impl PageSizeBounds {
    /// The page size a request asking for `requested` is answered with.
    #[must_use]
    pub const fn limit(&self, requested: Option<u32>) -> u32 {
        let asked = match requested {
            Some(limit) => limit,
            None => self.default,
        };
        match self.cap {
            PageSizeCap::Capped(cap) if asked > cap => cap,
            PageSizeCap::Capped(_) | PageSizeCap::Uncapped => asked,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::pagination::page_size::{PageSizeBounds, PageSizeCap};
    use pretty_assertions::assert_eq;

    const CAPPED: PageSizeBounds = PageSizeBounds {
        default: 20,
        cap: PageSizeCap::Capped(1000),
    };

    const UNCAPPED: PageSizeBounds = PageSizeBounds {
        default: 100,
        cap: PageSizeCap::Uncapped,
    };

    /// A request naming no `limit` gets the collection's default.
    #[test]
    fn the_default_applies_when_no_limit_is_asked() {
        assert_eq!(CAPPED.limit(None), 20);
        assert_eq!(UNCAPPED.limit(None), 100);
    }

    /// A capped collection lowers a larger request to its cap.
    #[test]
    fn the_cap_bounds_the_limit() {
        assert_eq!(CAPPED.limit(Some(5000)), 1000);
        assert_eq!(CAPPED.limit(Some(7)), 7);
    }

    /// An uncapped collection honours a large request.
    #[test]
    fn an_uncapped_collection_keeps_a_large_limit() {
        assert_eq!(UNCAPPED.limit(Some(5000)), 5000);
    }
}
