//! Which of the instances valid within a temporal window a probe reads.
//!
//! Clause 4.11 makes a temporal query select the values "valid within the
//! specified timeframe": the instances inside the interval and, for an
//! interval with a start, the instance each stream held when it opens (Annex
//! C clause C.5.5). A query whose entities are held whole reads both. A split
//! query (clause 5.7.4.4) reads the instances inside the window part by part,
//! but the instance in effect is the latest across every part, which no part
//! decides alone, so its leaves read the inside of the window only.

/// The instances valid within a window that a probe reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowReach {
    /// The instances inside the interval.
    Inside,
    /// The instances inside the interval and, for an interval with a start,
    /// the one each stream held when it opens.
    InsideAndInEffect,
}
