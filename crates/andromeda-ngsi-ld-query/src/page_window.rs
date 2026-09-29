//! The page of an identifier-ordered collection a request asks for (clause 5.5.9).
//!
//! Clause 5.5.9 offers two ways of pointing at a page: the opaque pointer of
//! clause 5.5.9.1, which Andromeda carries as a keyset [`PageCursor`], and the
//! transparent `limit`/`offset` pair of clause 5.5.9.2. A store pages its own
//! rows itself. A collection assembled in memory -- the local entities merged
//! with what context sources contributed (clause 5.7.2.4) -- is sorted by
//! identifier and cut here instead, so either pointer selects from it exactly
//! the elements it would have selected from a store holding the same rows.

use crate::cursor::PageCursor;

/// Where a page starts in an identifier-ordered collection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PageStart {
    /// Clause 5.5.9.2: skip this many elements of the collection.
    Offset(u32),
    /// Clause 5.5.9.1: continue from the opaque cursor the page was fetched
    /// under, or begin the walk when there is none.
    Cursor(Option<PageCursor>),
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
    /// Cut this window from `sorted`, a collection sorted ascending by the
    /// identifier `key` reads, compared byte-wise as the store collates
    /// identifiers.
    ///
    /// A forward cursor takes the elements past its boundary, a backward one the
    /// elements closest before it, and the from-end cursor the final elements:
    /// at most `limit` of them each time, in ascending order. These are the rows
    /// a store's keyset read returns once a descending read is reversed, so a
    /// walk moves between a stored and a merged collection without skipping or
    /// repeating an element.
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
        page_window::{PageStart, PageWindow},
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

    /// A forward cursor whose boundary left the collection still resumes at the
    /// next identifier that remains, so a deletion mid-walk skips nothing.
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

    /// Identifiers compare byte-wise, as the store collates them: `urn:e:10`
    /// sorts before `urn:e:9`, and a cursor honours that order.
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
}
