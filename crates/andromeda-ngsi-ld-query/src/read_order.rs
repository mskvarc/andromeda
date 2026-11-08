//! The order a store reads one page of an identifier-ordered collection in.

use crate::ordering::sort_direction::SortDirection;

/// The order a store reads a page's rows in, before they are presented.
///
/// Every page is presented ascending by identifier. A page that ends at a
/// boundary (the one before a backward cursor, or the final one) is read
/// descending instead, so a single index scan from that boundary serves it,
/// and is reversed to ascending once read
/// ([`crate::page_window::PageStart::read_order`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadOrder {
    /// Read in the order the page is presented in.
    Ascending,
    /// Read from the page's far end, then reversed for presentation.
    Descending,
}

impl ReadOrder {
    /// Put rows read in this order into the ascending order every page is
    /// presented in.
    pub const fn present<T>(self, rows: &mut [T]) {
        match self {
            ReadOrder::Ascending => {}
            ReadOrder::Descending => rows.reverse(),
        }
    }
}

// Hand-written because the mapping pairs two enums no derive relates.
impl From<ReadOrder> for SortDirection {
    fn from(order: ReadOrder) -> SortDirection {
        match order {
            ReadOrder::Ascending => SortDirection::Ascending,
            ReadOrder::Descending => SortDirection::Descending,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::read_order::ReadOrder;
    use pretty_assertions::assert_eq;

    /// An ascending read is already in presentation order.
    #[test]
    fn an_ascending_read_is_presented_as_read() {
        let mut rows = vec![1, 2, 3];
        ReadOrder::Ascending.present(&mut rows);
        assert_eq!(rows, vec![1, 2, 3]);
    }

    /// A descending read is reversed into presentation order.
    #[test]
    fn a_descending_read_is_reversed_for_presentation() {
        let mut rows = vec![3, 2, 1];
        ReadOrder::Descending.present(&mut rows);
        assert_eq!(rows, vec![1, 2, 3]);
    }
}
