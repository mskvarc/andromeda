//! The persisted distributed-operation mode of one registration.

use crate::{compact_code_registry, registry::UnknownCompactCode};

compact_code_registry! {
    /// The distributed-operation mode of a registration (clause 4.3.6, clause 5.2.9 `mode`).
    ///
    /// The labels are the specification's own mode tokens.
    CsrModeCode {
        frozen_prefix: 4,
        frozen_digest: 0xc3e8_b894_eb08_ceb5,
        codes: {
            /// The source adds to what the broker holds.
            Inclusive = 1, "inclusive";
            /// The source alone holds the registered information.
            Exclusive = 2, "exclusive";
            /// Requests are redirected to the source.
            Redirect = 3, "redirect";
            /// The source is consulted only for what nothing else holds.
            Auxiliary = 4, "auxiliary";
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::csr_mode::CsrModeCode;
    use pretty_assertions::assert_eq;

    #[test]
    fn the_members_are_numbered_as_the_migration_documents_them() {
        assert_eq!(CsrModeCode::ALL.len(), 4);
        assert_eq!(CsrModeCode::Inclusive.code(), 1);
        assert_eq!(CsrModeCode::Auxiliary.code(), 4);
    }

    #[test]
    fn zero_and_the_number_after_the_last_member_fail_to_decode() {
        assert!(CsrModeCode::try_from(0).is_err());
        assert!(CsrModeCode::try_from(5).is_err());
    }
}
