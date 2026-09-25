//! The persisted health of one registration's context source.

use crate::{compact_code_registry, registry::UnknownCompactCode};

compact_code_registry! {
    /// The health of a registration's context source (clause 5.2.9 `status`).
    ///
    /// The labels are the specification's own status tokens.
    CsrStatusCode {
        frozen_prefix: 2,
        frozen_digest: 0xad70_9d61_3ed2_a231,
        codes: {
            /// The last forwarded request succeeded.
            Ok = 1, "ok";
            /// The last forwarded request failed.
            Failed = 2, "failed";
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::csr_status::CsrStatusCode;
    use pretty_assertions::assert_eq;

    #[test]
    fn the_members_are_numbered_as_the_migration_documents_them() {
        assert_eq!(CsrStatusCode::ALL.len(), 2);
        assert_eq!(CsrStatusCode::Ok.code(), 1);
        assert_eq!(CsrStatusCode::Failed.code(), 2);
    }

    #[test]
    fn zero_and_the_number_after_the_last_member_fail_to_decode() {
        assert!(CsrStatusCode::try_from(0).is_err());
        assert!(CsrStatusCode::try_from(3).is_err());
    }
}
