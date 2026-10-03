//! Whether a returned page filled the window it was cut by (clause 6.3.10).

/// Whether a page came back holding as many elements as its window asked for.
///
/// A collection walked without a total -- every cursor walk, and an offset
/// query that did not ask for a count -- has no other way to tell that a
/// further page exists: clause 6.3.10 offers the `next` pointer exactly while
/// one does, and a page that filled its limit is the only evidence a count-less
/// read has of that. [`crate::page_window::PageWindow::fullness`] is the one
/// place the comparison is made, so no collection can drift to a different
/// reading of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageFullness {
    /// The page returned its whole limit, so a following page may exist.
    Full,
    /// The page returned fewer elements than its limit, so it reached the end
    /// of the collection in the direction it was read.
    Short,
}
