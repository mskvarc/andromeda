//! The most bytes one page of a collection may render to.
//!
//! ETSI GS CIM 009 v1.9.1 clause 5.5.6 raises `TooManyResults` for a request
//! the broker will not serve and leaves the threshold to the broker. The count
//! of a page is bounded by its `limit`, but a count says nothing of what the
//! elements cost once rendered: a thousand entities of a few attributes and a
//! thousand of a few thousand each differ by orders of magnitude. The ceiling
//! is therefore on the rendered bytes, and a page over it is refused whole
//! instead of answered with a smaller page, since clause 5.5.9.1 makes the page
//! size the client's requested maximum.

use crate::classified_report::ClassifiedReport;
use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
use thiserror::Error;

/// The most bytes one page of a collection may render to.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PageByteCeiling(usize);

impl PageByteCeiling {
    /// A ceiling of `bytes`.
    #[must_use]
    pub const fn new(bytes: usize) -> PageByteCeiling {
        PageByteCeiling(bytes)
    }

    /// Admit a rendered page that fits the ceiling.
    ///
    /// # Errors
    ///
    /// Returns [`PageTooLarge`] for a page that renders to more bytes than the
    /// ceiling.
    pub const fn admit(self, size: RenderedPageSize) -> Result<(), PageTooLarge> {
        if size.0 > self.0 {
            return Err(PageTooLarge { size: size.0, ceiling: self.0 });
        }
        Ok(())
    }
}

/// The bytes a collection response's body holds, carried on the response so
/// the stack can hold it against the ceiling without reading the body.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RenderedPageSize(usize);

impl RenderedPageSize {
    /// A page of `bytes`.
    #[must_use]
    pub const fn new(bytes: usize) -> RenderedPageSize {
        RenderedPageSize(bytes)
    }
}

/// A page that renders to more bytes than the ceiling admits.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
#[error("the page renders to {size} bytes, more than the {ceiling} bytes one page may hold; ask for a smaller limit and follow the next page pointer")]
pub struct PageTooLarge {
    /// The bytes the page renders to.
    size: usize,
    /// The bytes one page may hold.
    ceiling: usize,
}

impl PageTooLarge {
    /// The clause 5.5.6 `TooManyResults` report of this refusal.
    #[must_use]
    pub fn report(self) -> ClassifiedReport {
        ClassifiedReport::public(NgsiLdErrorType::TooManyResults, self.to_string())
    }
}

#[cfg(test)]
mod tests {
    use crate::page_ceiling::{PageByteCeiling, PageTooLarge, RenderedPageSize};
    use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;

    #[test]
    fn a_page_exactly_at_the_ceiling_is_admitted() {
        assert_eq!(PageByteCeiling::new(100).admit(RenderedPageSize::new(100)), Ok(()));
    }

    #[test]
    fn a_page_one_byte_over_the_ceiling_is_refused_with_both_sizes() {
        let refused = PageByteCeiling::new(100).admit(RenderedPageSize::new(101));
        assert_eq!(refused, Err(PageTooLarge { size: 101, ceiling: 100 }));
    }

    #[test]
    fn the_refusal_is_a_too_many_results_naming_the_size_and_the_ceiling() {
        let refused = PageByteCeiling::new(100).admit(RenderedPageSize::new(101)).expect_err("101 bytes exceed 100");
        pretty_assertions::assert_eq!(refused.report().error_type(), NgsiLdErrorType::TooManyResults);
        pretty_assertions::assert_eq!(
            refused.to_string(),
            "the page renders to 101 bytes, more than the 100 bytes one page may hold; ask for a smaller limit and follow the next page pointer"
        );
    }
}
