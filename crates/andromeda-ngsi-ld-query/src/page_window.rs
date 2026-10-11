//! The page of an identifier-ordered collection a request asks for (clause 5.5.9).
//!
//! Clause 5.5.9 offers the opaque pointer of clause 5.5.9.1, carried as a keyset [`PageCursor`], and the transparent `limit`/`offset` pair of clause 5.5.9.2. The pagination policy at the request boundary decides which one a request walks by, and the resulting [`PageWindow`] drives every store and every in-memory cut. A store pages its own rows; a collection assembled in memory (local entities merged with what context sources contributed, clause 5.7.2.4) is sorted by identifier and cut here, so either pointer selects from it exactly what it would select from a store holding the same rows.

use crate::{cursor::PageCursor, page_reach::PageReach, read_order::ReadOrder};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;

/// Where a page starts in an identifier-ordered collection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PageStart {
    /// Clause 5.5.9.2: skip this many elements.
    Offset(u32),
    /// Clause 5.5.9.1: continue from the opaque cursor, or begin the walk when there is none.
    Cursor(Option<PageCursor>),
}

impl PageStart {
    /// The order a store reads the page in. A backward cursor's page ends at its boundary and the from-end page at the end of the collection, so both are read descending from that end and reversed for presentation.
    #[must_use]
    pub const fn read_order(&self) -> ReadOrder {
        match self {
            PageStart::Cursor(Some(PageCursor::Before(_) | PageCursor::End)) => ReadOrder::Descending,
            PageStart::Offset(_) | PageStart::Cursor(None | Some(PageCursor::After(_))) => ReadOrder::Ascending,
        }
    }
}

/// The page a request asks for: how many elements, and from where.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageWindow {
    /// The clause 6.3.10 page size.
    pub limit: u32,
    /// Where the page starts.
    pub start: PageStart,
}

impl PageWindow {
    /// The clause 5.5.9.2 window of `limit` elements after skipping `offset`.
    #[must_use]
    pub const fn offset(limit: u32, offset: u32) -> PageWindow {
        PageWindow {
            limit,
            start: PageStart::Offset(offset),
        }
    }

    /// The whole collection, for a reader that needs every match rather than one page.
    #[must_use]
    pub const fn unbounded() -> PageWindow {
        PageWindow::offset(u32::MAX, 0)
    }

    /// The `limit` elements that follow `boundary` in identifier order, the step of an internal keyset walk that must neither skip nor repeat an element while the collection changes.
    #[must_use]
    pub const fn after(limit: u32, boundary: Identifier) -> PageWindow {
        PageWindow {
            limit,
            start: PageStart::Cursor(Some(PageCursor::After(boundary))),
        }
    }

    /// The window a reader reads for this page: one element more, the one past the page in the direction it is read, whose presence is what tells whether the collection continues there (clause 5.5.9.1). [`PageWindow::settle`] takes it off again.
    #[must_use]
    pub fn with_lookahead(&self) -> PageWindow {
        PageWindow {
            limit: self.limit.saturating_add(1),
            // clone: the read window starts where the page does
            start: self.start.clone(),
        }
    }

    /// This page of `read`, the elements read through [`PageWindow::with_lookahead`] in the order the page is presented in, and whether the collection continues past it in the direction it was read.
    ///
    /// The element past the page sits at the end it was read towards: the last of a page read ascending, the first of one read descending and reversed for presentation.
    #[must_use]
    pub fn settle<T>(&self, read: Vec<T>) -> (Vec<T>, PageReach) {
        let limit = usize::try_from(self.limit).unwrap_or(usize::MAX);
        let surplus = read.len().saturating_sub(limit);
        if surplus == 0 {
            return (read, PageReach::Exhausted);
        }
        let mut read = read;
        match self.start.read_order() {
            ReadOrder::Ascending => read.truncate(limit),
            ReadOrder::Descending => {
                read.drain(..surplus);
            }
        }
        (read, PageReach::Continues)
    }

    /// Cuts this page from `sorted` as [`PageWindow::cut`] does, together with whether the collection continues past it.
    #[must_use]
    pub fn cut_settled<T>(&self, sorted: Vec<T>, key: impl Fn(&T) -> &str) -> (Vec<T>, PageReach) {
        self.settle(self.with_lookahead().cut(sorted, key))
    }

    /// Cuts this window from `sorted`, ascending by the identifier `key` reads, compared byte-wise as the store collates identifiers.
    ///
    /// An offset needs no particular order. A forward cursor takes the elements past its boundary, a backward one the elements closest before it, and the from-end cursor the final elements: at most `limit`, ascending. These are the rows a store's keyset read returns once a descending read is reversed, so a walk moves between a stored and a merged collection without skipping or repeating an element.
    #[must_use]
    pub fn cut<T>(&self, sorted: Vec<T>, key: impl Fn(&T) -> &str) -> Vec<T> {
        let limit = usize::try_from(self.limit).unwrap_or(usize::MAX);
        let len = sorted.len();
        let forward_from = |start: usize| (start, start.saturating_add(limit).min(len));
        let (start, end) = match &self.start {
            PageStart::Offset(offset) => forward_from(usize::try_from(*offset).unwrap_or(usize::MAX).min(len)),
            PageStart::Cursor(None) => forward_from(0),
            PageStart::Cursor(Some(PageCursor::After(boundary))) => forward_from(sorted.partition_point(|element| key(element) <= boundary.as_str())),
            PageStart::Cursor(Some(PageCursor::Before(boundary))) => {
                let end = sorted.partition_point(|element| key(element) < boundary.as_str());
                (end.saturating_sub(limit), end)
            }
            PageStart::Cursor(Some(PageCursor::End)) => (len.saturating_sub(limit), len),
        };
        sorted.into_iter().skip(start).take(end - start).collect()
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        cursor::PageCursor,
        page_reach::PageReach,
        page_window::{PageStart, PageWindow},
        read_order::ReadOrder,
    };
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use pretty_assertions::assert_eq;

    fn uri(value: &str) -> Identifier {
        value.parse().expect("valid URI")
    }

    /// Ten identifiers, `urn:e:0` to `urn:e:9`, already in identifier order.
    fn collection() -> Vec<String> {
        (0..10).map(|index| format!("urn:e:{index}")).collect()
    }

    fn cut(limit: u32, start: PageStart) -> Vec<String> {
        PageWindow { limit, start }.cut(collection(), String::as_str)
    }

    fn ids(indices: &[u8]) -> Vec<String> {
        indices.iter().map(|index| format!("urn:e:{index}")).collect()
    }

    /// Clause 5.5.9.2: an offset skips that many elements and takes the limit.
    #[test]
    fn an_offset_window_skips_then_takes_the_limit() {
        assert_eq!(cut(3, PageStart::Offset(4)), ids(&[4, 5, 6]));
    }

    /// An offset past the end selects nothing rather than failing.
    #[test]
    fn an_offset_past_the_end_selects_nothing() {
        assert_eq!(cut(3, PageStart::Offset(40)), Vec::<String>::new());
    }

    /// The first page of a cursor walk is the start of the collection.
    #[test]
    fn a_fresh_cursor_walk_starts_at_the_beginning() {
        assert_eq!(cut(3, PageStart::Cursor(None)), ids(&[0, 1, 2]));
    }

    /// A forward cursor takes the elements strictly after its boundary.
    #[test]
    fn a_forward_cursor_takes_the_elements_after_its_boundary() {
        assert_eq!(cut(3, PageStart::Cursor(Some(PageCursor::After(uri("urn:e:2"))))), ids(&[3, 4, 5]));
    }

    /// The boundary may have left the collection; a deletion mid-walk must skip nothing.
    #[test]
    fn a_forward_cursor_resumes_past_a_boundary_that_left_the_collection() {
        let remaining: Vec<String> = collection().into_iter().filter(|id| id != "urn:e:3").collect();
        let page = PageWindow {
            limit: 2,
            start: PageStart::Cursor(Some(PageCursor::After(uri("urn:e:3")))),
        }
        .cut(remaining, String::as_str);
        assert_eq!(page, ids(&[4, 5]));
    }

    /// A forward cursor near the end returns the short final page.
    #[test]
    fn a_forward_cursor_near_the_end_returns_a_short_page() {
        assert_eq!(cut(3, PageStart::Cursor(Some(PageCursor::After(uri("urn:e:8"))))), ids(&[9]));
    }

    /// A backward cursor takes the elements closest before its boundary, in
    /// ascending order.
    #[test]
    fn a_backward_cursor_takes_the_closest_elements_before_its_boundary() {
        assert_eq!(cut(3, PageStart::Cursor(Some(PageCursor::Before(uri("urn:e:6"))))), ids(&[3, 4, 5]));
    }

    /// A backward cursor near the start returns the short first page.
    #[test]
    fn a_backward_cursor_near_the_start_returns_a_short_page() {
        assert_eq!(cut(3, PageStart::Cursor(Some(PageCursor::Before(uri("urn:e:1"))))), ids(&[0]));
    }

    /// The from-end cursor takes the final elements, in ascending order.
    #[test]
    fn the_from_end_cursor_takes_the_final_elements() {
        assert_eq!(cut(3, PageStart::Cursor(Some(PageCursor::End))), ids(&[7, 8, 9]));
    }

    /// A limit larger than the collection takes all of it, in every direction.
    #[test]
    fn a_limit_larger_than_the_collection_takes_all_of_it() {
        assert_eq!(cut(40, PageStart::Cursor(Some(PageCursor::End))), collection());
        assert_eq!(cut(40, PageStart::Cursor(None)), collection());
    }

    /// Identifiers compare byte-wise as the store collates them: `urn:e:10` sorts before `urn:e:9`.
    #[test]
    fn identifiers_compare_byte_wise() {
        let mut sorted = vec!["urn:e:9".to_string(), "urn:e:10".to_string(), "urn:e:1".to_string()];
        sorted.sort();
        let sorted = sorted;
        let page = PageWindow {
            limit: 5,
            start: PageStart::Cursor(Some(PageCursor::After(uri("urn:e:1")))),
        }
        .cut(sorted, String::as_str);
        assert_eq!(page, vec!["urn:e:10".to_string(), "urn:e:9".to_string()]);
    }

    /// The read window asks for one element past the page, from where the page starts.
    #[test]
    fn the_lookahead_reads_one_element_past_the_page() {
        let window = PageWindow::after(3, uri("urn:e:2"));
        assert_eq!(window.with_lookahead(), PageWindow::after(4, uri("urn:e:2")));
    }

    /// The unbounded window has no element past it to read.
    #[test]
    fn the_unbounded_window_reads_no_lookahead() {
        assert_eq!(PageWindow::unbounded().with_lookahead(), PageWindow::unbounded());
    }

    /// A forward page whose lookahead came back continues past its last element.
    #[test]
    fn a_forward_page_with_its_lookahead_continues_past_its_end() {
        let window = PageWindow::after(2, uri("urn:e:0"));
        assert_eq!(window.settle(ids(&[1, 2, 3])), (ids(&[1, 2]), PageReach::Continues));
    }

    /// A full forward page that read no lookahead ends the collection.
    #[test]
    fn a_full_forward_page_without_its_lookahead_is_exhausted() {
        let window = PageWindow::after(2, uri("urn:e:7"));
        assert_eq!(window.settle(ids(&[8, 9])), (ids(&[8, 9]), PageReach::Exhausted));
    }

    /// Clause 5.5.9.1: a backward page is read towards the start, so its lookahead is the element before its first one, and it is presented first.
    #[test]
    fn a_backward_page_drops_the_lookahead_before_its_first_element() {
        let window = PageWindow {
            limit: 2,
            start: PageStart::Cursor(Some(PageCursor::Before(uri("urn:e:5")))),
        };
        assert_eq!(window.settle(ids(&[2, 3, 4])), (ids(&[3, 4]), PageReach::Continues));
    }

    /// Clause 5.5.9.1: a full backward page holding the first elements has no previous elements.
    #[test]
    fn a_full_backward_page_holding_the_first_elements_is_exhausted() {
        let page = PageWindow {
            limit: 2,
            start: PageStart::Cursor(Some(PageCursor::Before(uri("urn:e:2")))),
        }
        .cut_settled(collection(), String::as_str);
        assert_eq!(page, (ids(&[0, 1]), PageReach::Exhausted));
    }

    /// The from-end page is read towards the start too.
    #[test]
    fn the_from_end_page_continues_while_elements_precede_it() {
        let window = PageWindow {
            limit: 3,
            start: PageStart::Cursor(Some(PageCursor::End)),
        };
        assert_eq!(window.cut_settled(collection(), String::as_str), (ids(&[7, 8, 9]), PageReach::Continues));
    }

    /// An offset page is read in the order it is presented, so its lookahead is its last element.
    #[test]
    fn an_offset_page_drops_its_trailing_lookahead() {
        assert_eq!(
            PageWindow::offset(3, 4).cut_settled(collection(), String::as_str),
            (ids(&[4, 5, 6]), PageReach::Continues)
        );
        assert_eq!(
            PageWindow::offset(3, 7).cut_settled(collection(), String::as_str),
            (ids(&[7, 8, 9]), PageReach::Exhausted)
        );
    }

    /// A backward cursor and the from-end cursor are read descending from the
    /// boundary they end at.
    #[test]
    fn backward_and_from_end_starts_read_descending() {
        assert_eq!(PageStart::Cursor(Some(PageCursor::Before(uri("urn:e:5")))).read_order(), ReadOrder::Descending);
        assert_eq!(PageStart::Cursor(Some(PageCursor::End)).read_order(), ReadOrder::Descending);
    }

    /// A forward cursor, an offset and a fresh walk are read as presented.
    #[test]
    fn forward_offset_and_fresh_starts_read_ascending() {
        assert_eq!(PageStart::Cursor(Some(PageCursor::After(uri("urn:e:5")))).read_order(), ReadOrder::Ascending);
        assert_eq!(PageStart::Offset(4).read_order(), ReadOrder::Ascending);
        assert_eq!(PageStart::Cursor(None).read_order(), ReadOrder::Ascending);
    }

    /// The unbounded window cuts nothing from the collection.
    #[test]
    fn unbounded_window_cuts_nothing() {
        assert_eq!(PageWindow::unbounded().cut(collection(), String::as_str), collection());
    }

    /// An internal keyset step continues strictly after its boundary.
    #[test]
    fn an_after_window_continues_past_its_boundary() {
        assert_eq!(PageWindow::after(2, uri("urn:e:6")).cut(collection(), String::as_str), ids(&[7, 8]));
    }
}
