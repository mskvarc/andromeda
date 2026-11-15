//! Which of the Scope events valid within a temporal window a probe reads.
//!
//! Clause 4.18 makes the Scope in effect when a window opens part of the
//! window's `scopeQ` reading (Annex C clause C.5.16): the events inside the
//! interval and, for an interval with a start, the one in force when it opens.
//! A query whose entities are held whole reads both. A split query (clause
//! 5.7.4.4) reads the events inside the window part by part, but the Scope in
//! effect is the latest across every part, which no part decides alone, so its
//! leaves read the inside of the window only.

/// The Scope events valid within a window that a probe reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowReach {
    /// The events inside the interval.
    Inside,
    /// The events inside the interval and, for an interval with a start,
    /// the one in force when it opens.
    InsideAndInEffect,
}
