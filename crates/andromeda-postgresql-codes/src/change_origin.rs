//! The persisted origin of one change a respond-async job owes.

use crate::{compact_code_domain::CompactCodeDomain, compact_code_registry, registry::UnknownCompactCode};

compact_code_registry! {
    /// Where the change an owed notification reports came from.
    ///
    /// Clause 5.8.6 notifies a change whichever broker stores the Entity, and
    /// the notification of a forwarded change is built from what the context
    /// source answered rather than from a local Entity.
    ChangeOriginCode {
        frozen_prefix: 2,
        frozen_digest: 0x367f_2838_f6a4_8499,
        codes: {
            /// The Entity is stored and managed by this broker.
            Local = 1, "local";
            /// The change was forwarded from a registered context source.
            Forwarded = 2, "forwarded";
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::change_origin::ChangeOriginCode;
    use pretty_assertions::assert_eq;

    #[test]
    fn the_origins_are_numbered_as_the_migration_writes_them() {
        assert_eq!(ChangeOriginCode::Local.code(), 1);
        assert_eq!(ChangeOriginCode::Forwarded.code(), 2);
    }

    #[test]
    fn an_origin_a_newer_release_might_add_fails_to_decode() {
        assert!(ChangeOriginCode::try_from(0).is_err());
        assert!(ChangeOriginCode::try_from(3).is_err());
    }
}
