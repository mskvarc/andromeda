//! Whether a collection continues past a page in the direction it was read (clause 5.5.9.1).

/// Whether elements of the collection lie past a page, at the end it was read towards.
///
/// Clause 5.5.9.1 offers the pointer to the following page while elements remain to be retrieved, and the pointer to the previous page on every page but the one holding the first elements. A page is read one element past its limit ([`crate::page_window::PageWindow::with_lookahead`]), so whether that element came back is known rather than guessed from how full the page is: a full page that ends the collection points nowhere further. [`crate::page_window::PageWindow::settle`] is the single place it is decided.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageReach {
    /// An element lies past the page in the direction it was read.
    Continues,
    /// The page reached the end of the collection in the direction it was read.
    Exhausted,
}
