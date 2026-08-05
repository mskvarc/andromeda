//! Whether a representation carries its system-generated attributes.

/// Whether system-generated attributes (`createdAt`, `modifiedAt`) should be
/// included in the response or stripped before serialization.
///
/// A named policy rather than a bare `bool`: it is threaded through query
/// compilation, rendering and notification payload construction, and at each of
/// those a boolean would read as "some unexplained flag".
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SysAttrsInclusion {
    /// Include `createdAt`, `modifiedAt`, and related system attributes.
    Include,
    /// Strip system attributes from the response.
    #[default]
    Strip,
}
