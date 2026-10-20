//! How many elements a page of one collection holds (clause 6.3.10 `limit`).

use thiserror::Error;

/// The largest page any paged collection of the broker answers with.
///
/// Clause 5.5.6 leaves the threshold of `TooManyResults` to the broker; one
/// thousand elements bounds a single response while a client walks a larger
/// collection page by page.
pub const BROKER_PAGE_SIZE_CAP: PageSizeCap = PageSizeCap::Capped(1000);

/// The page size a collection answers with when the request names no `limit`,
/// and the most it answers with when the request names one.
///
/// Clause 5.5.6 makes both a broker decision, and the default differs per
/// collection.
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
    /// A requested `limit` above this value is refused.
    Capped(u32),
    /// A requested `limit` is honoured as asked.
    Uncapped,
}

/// A requested `limit` above the collection's cap.
///
/// Clause 5.5.9.1 makes the page size the client's requested maximum, and only
/// the response payload size may lower it, so a request the broker will not
/// serve whole is refused with `TooManyResults` (clause 5.5.6) rather than
/// answered with a smaller page.
#[derive(Debug, Clone, Copy, Error, PartialEq, Eq)]
#[error("limit={requested} asks for more than the {cap} elements one page holds; ask for at most {cap} and follow the next page pointer")]
pub struct PageSizeExceeded {
    /// The `limit` the request named.
    pub requested: u32,
    /// The most elements one page of the collection holds.
    pub cap: u32,
}

impl PageSizeBounds {
    /// The page size a request asking for `requested` is answered with.
    ///
    /// # Errors
    ///
    /// Returns [`PageSizeExceeded`] when `requested` is above the cap.
    pub const fn limit(&self, requested: Option<u32>) -> Result<u32, PageSizeExceeded> {
        let asked = match requested {
            Some(limit) => limit,
            None => self.default,
        };
        match self.cap {
            PageSizeCap::Capped(cap) if asked > cap => Err(PageSizeExceeded { requested: asked, cap }),
            PageSizeCap::Capped(_) | PageSizeCap::Uncapped => Ok(asked),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::pagination::page_size::{PageSizeBounds, PageSizeCap, PageSizeExceeded};
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
        assert_eq!(CAPPED.limit(None), Ok(20));
        assert_eq!(UNCAPPED.limit(None), Ok(100));
    }

    /// Clause 5.5.9.1: L equals the requested maximum up to the cap itself.
    #[test]
    fn a_limit_up_to_the_cap_is_served_as_asked() {
        assert_eq!(CAPPED.limit(Some(7)), Ok(7));
        assert_eq!(CAPPED.limit(Some(1000)), Ok(1000));
    }

    /// Clause 5.5.6: a `limit` above the cap is refused, naming the cap.
    #[test]
    fn a_limit_above_the_cap_is_refused_naming_the_cap() {
        assert_eq!(CAPPED.limit(Some(1001)), Err(PageSizeExceeded { requested: 1001, cap: 1000 }));
        assert_eq!(CAPPED.limit(Some(5000)), Err(PageSizeExceeded { requested: 5000, cap: 1000 }));
    }

    #[test]
    fn the_refusal_names_the_requested_limit_and_the_cap() {
        let message = PageSizeExceeded { requested: 5000, cap: 1000 }.to_string();
        assert_eq!(
            message,
            "limit=5000 asks for more than the 1000 elements one page holds; ask for at most 1000 and follow the next page pointer"
        );
    }

    /// An uncapped collection honours a large request.
    #[test]
    fn an_uncapped_collection_keeps_a_large_limit() {
        assert_eq!(UNCAPPED.limit(Some(5000)), Ok(5000));
    }
}
