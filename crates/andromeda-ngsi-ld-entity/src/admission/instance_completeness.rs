//! Whether a written Attribute Fragment must stand as a complete instance.
//!
//! Clause 5.5.8 has a partial update change "only the elements provided", so
//! its Fragment may name any members, or none (clause 5.4, RFC 7396). Clause
//! 5.6.19.4 has Replace Attribute replace the instance with the Fragment, which
//! is therefore an instance of its own: Table 5.2.5-1 and its siblings make the
//! value member mandatory.

/// What the operation writing a bare Attribute Fragment requires of it.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum InstanceCompleteness {
    /// The Fragment replaces an instance (clause 5.6.19), so it carries the
    /// value member its family requires; a Fragment declaring no family is
    /// read as a Property (clause 4.5.2).
    Complete,
    /// The Fragment patches the members it names into the instance it targets
    /// (clauses 5.5.8 and 5.6.4), so it may leave any member out.
    Partial,
}
