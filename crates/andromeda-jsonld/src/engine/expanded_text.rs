//! An expanded document handed to compaction as JSON text, named by who
//! expanded it.
//!
//! Compaction works on the processor's own expanded form, so every document is
//! first read into it. How it is read depends on where the document came from:
//! one this broker's own expansion produced -- a stored payload, or a document
//! just expanded -- is canonical expanded JSON-LD and is read as it stands,
//! while one assembled in memory or received from another party is taken
//! through the expansion algorithm, which normalizes it.

/// The JSON text of an expanded JSON-LD document to compact, with its
/// provenance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExpandedText<'a> {
    /// A document this broker's own expansion produced, as the store keeps it
    /// or as expansion just returned it: every member value is an array and
    /// every member key is a keyword or an absolute IRI, so it is read
    /// directly, without expanding it again.
    Canonical(&'a [u8]),
    /// A document built in memory or received from elsewhere, which may carry
    /// a lone value where expanded form has an array, or a key that is not an
    /// absolute IRI; expansion normalizes both before compaction.
    Assembled(&'a [u8]),
}
