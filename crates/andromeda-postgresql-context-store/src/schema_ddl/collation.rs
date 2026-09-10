//! `CREATE COLLATION` for the ICU collations clause 4.23.1 admits.
//!
//! `CREATE COLLATION` is a whole administrative command with no bound
//! parameters, so `SeaQuery` does not model it; it is spelled here, in the one
//! module sanctioned to write dialect SQL by hand.
//!
//! PostgreSQL pre-imports one collation object per ICU locale, each named
//! `<locale>-x-icu`, which covers the clause 4.23.1 default `und-x-icu`. It does
//! not pre-import the keyword forms RFC 6067 defines, so the collations the
//! clause's own examples use -- `und-u-ks-identic` (EXAMPLE 6) and
//! `de-u-co-phonebk` (EXAMPLE 7) -- have no object until one is created, and
//! `ORDER BY … COLLATE "de-u-co-phonebk"` fails with `collation … does not
//! exist`. Creating it on first use is what lets a conformant request succeed
//! rather than answering 500 to a value the specification names.

use andromeda_ngsi_ld_query::ordering::collation::IcuCollation;

/// The `CREATE COLLATION IF NOT EXISTS "<name>" (provider = icu, locale = '<tag>')` statement.
///
/// The name and the locale are both the collation identifier, which reached
/// here only after [`IcuCollation::parse`] accepted it as a well-formed BCP47
/// tag whose Unicode keywords are collation keywords -- so it is drawn from
/// `[A-Za-z0-9-]` and carries neither a quote nor a backslash. Validity is
/// decided there rather than by PostgreSQL, because `icu_validation_level`
/// defaults to `warning`: an unknown locale would otherwise create a usable
/// object that silently sorts by the root collation instead of being refused.
#[must_use]
pub fn create_icu_collation(collation: &IcuCollation) -> String {
    let tag = collation.as_str();
    format!("CREATE COLLATION IF NOT EXISTS \"{tag}\" (provider = icu, locale = '{tag}')")
}

#[cfg(test)]
mod tests {
    use crate::schema_ddl::collation::create_icu_collation;
    use andromeda_ngsi_ld_query::ordering::collation::IcuCollation;

    fn collation(raw: &str) -> IcuCollation {
        IcuCollation::parse(raw).expect("the fixture is a valid ICU collation")
    }

    /// Clause 4.23.1 EXAMPLE 7 names `de-u-co-phonebk`, which PostgreSQL does
    /// not pre-import; the statement provisions it under its own name.
    #[test]
    fn a_keyword_collation_is_created_under_its_own_name() {
        assert_eq!(
            create_icu_collation(&collation("de-u-co-phonebk")),
            "CREATE COLLATION IF NOT EXISTS \"de-u-co-phonebk\" (provider = icu, locale = 'de-u-co-phonebk')"
        );
    }

    /// The clause 4.23.1 default is pre-imported, but creating it is still a
    /// no-op rather than an error, so the caller needs no special case.
    #[test]
    fn the_root_collation_is_created_if_not_exists() {
        assert_eq!(
            create_icu_collation(&collation(IcuCollation::ROOT)),
            "CREATE COLLATION IF NOT EXISTS \"und-x-icu\" (provider = icu, locale = 'und-x-icu')"
        );
    }

    /// A validated collation identifier carries no character that could close
    /// either the quoted identifier or the string literal.
    #[test]
    fn a_validated_identifier_carries_no_quote_or_backslash() {
        for raw in ["und-x-icu", "de-u-co-phonebk", "und-u-ks-identic", "de-DE"] {
            let rendered = create_icu_collation(&collation(raw));
            assert_eq!(rendered.matches('"').count(), 2, "{rendered}");
            assert_eq!(rendered.matches('\'').count(), 2, "{rendered}");
            assert!(!rendered.contains('\\'), "{rendered}");
        }
    }
}
