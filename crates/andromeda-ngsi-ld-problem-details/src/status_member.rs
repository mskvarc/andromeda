//! Whether a problem document states the HTTP status of what it reports.
//!
//! Clause 5.5.3 requires only `type`, `title` and `detail` of an error payload,
//! and IETF RFC 7807 section 3.1 makes `status` advisory: it repeats the status
//! of the response the problem is the body of. A problem that accompanies a
//! result which is not an error response, such as the execution result of a
//! Snapshot query that succeeded without a Context Source (clause 5.2.42),
//! has no such status to repeat.

/// Whether a problem document carries the `status` member.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum StatusMember {
    /// The document carries `status`, the HTTP status its classification
    /// binds.
    #[default]
    Stated,
    /// The document carries no `status`: it accompanies a result that is not
    /// an error response.
    Omitted,
}
