//! Where a temporal query's `lastN` limit came from.

/// Indicates the origin of the `lastN` limit in a temporal query.
///
/// This enum replaces two boolean fields (`system_last_n` and
/// `user_specified_last_n`) to clearly distinguish between the four possible
/// states of lastN handling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LastNOrigin {
    /// The user explicitly specified a `lastN` parameter within the system cap.
    /// No truncation detection needed -- the user gets exactly what they asked for.
    UserSpecified {
        /// The count the client asked for.
        ///
        /// Clause 6.3.10 makes the `Content-Range` `size` of a backwards page
        /// "equal to the requested `lastN`", so the request's own number has to
        /// survive compilation rather than being replaced by what was found.
        requested: u32,
    },
    /// The user specified `lastN` but it exceeded the system cap, so the system
    /// reduced it. Triggers truncation detection (206 Partial Content) but uses
    /// user-specified direction for Content-Range.
    UserSpecifiedCapped {
        /// The count the client asked for, before the broker's safety cap narrowed it.
        requested: u32,
    },
    /// No user `lastN` was specified; the system applied `DEFAULT_LAST_N` as a cap.
    /// Triggers truncation detection (206 Partial Content) with system direction
    /// for Content-Range.
    SystemCapped,
    /// No `lastN` limit is active (e.g. aggregated queries without lastN).
    #[default]
    None,
}

impl LastNOrigin {
    /// Returns true when the system cap is active and truncation may occur.
    /// This is the case for both `SystemCapped` and `UserSpecifiedCapped`.
    #[must_use]
    pub const fn is_system_capped(self) -> bool {
        matches!(self, LastNOrigin::SystemCapped | LastNOrigin::UserSpecifiedCapped { .. })
    }

    /// Returns true when the user explicitly provided a `lastN` parameter,
    /// regardless of whether it was further capped by the system.
    #[must_use]
    pub const fn is_user_specified(self) -> bool {
        matches!(self, LastNOrigin::UserSpecified { .. } | LastNOrigin::UserSpecifiedCapped { .. })
    }

    /// The `lastN` the client asked for, when the client asked for one.
    ///
    /// Clause 6.3.10 reports this, not the number of instances actually found,
    /// as the `Content-Range` `size` of a backwards page.
    #[must_use]
    pub const fn requested(self) -> Option<u32> {
        match self {
            LastNOrigin::UserSpecified { requested } | LastNOrigin::UserSpecifiedCapped { requested } => Some(requested),
            LastNOrigin::SystemCapped | LastNOrigin::None => Option::None,
        }
    }
}
