//! `CREATE COLLATION` for the ICU collations clause 4.23.1 admits.

use andromeda_ngsi_ld_query::ordering::collation::IcuCollation;

/// `SET LOCAL icu_validation_level = 'error'`, the level the `CREATE COLLATION`
/// below is issued under.
///
/// The parameter defaults to `warning`, at which PostgreSQL creates a usable
/// collation object for a locale ICU does not know and every ordering under it
/// silently falls back to the root collation — a successful response in an
/// order the client never asked for. At `error` the locale is refused instead,
/// which clauses 5.7.2.4 and 5.7.4.4 make the client's fault: "if a preferred
/// collation setting is present and it does not conform to a valid ICU
/// collation (see IETF RFC 6067) then an error of type `BadRequestData` shall be
/// raised".
///
/// `SET LOCAL` rather than `SET`: the level applies to the transaction that
/// creates the collation and is gone when it ends, so the connection returns to
/// the pool exactly as it was drawn from it.
pub const STRICT_ICU_LOCALE_VALIDATION: &str = "SET LOCAL icu_validation_level = 'error'";

/// The `CREATE COLLATION IF NOT EXISTS "<name>" (provider = icu, locale = '<tag>')` statement.
///
/// The name and the locale are both the collation identifier, which reached
/// here only after [`IcuCollation::parse`] accepted it as a well-formed BCP47
/// tag whose Unicode keywords are the collation keys and types IETF RFC 6067
/// registers -- so it is drawn from `[A-Za-z0-9-]` and carries neither a quote
/// nor a backslash.
///
/// That parse decides what RFC 6067 governs and nothing else: whether the
/// locale names a collation ICU actually holds is ICU's own question, and it is
/// answered here, under [`STRICT_ICU_LOCALE_VALIDATION`].
#[must_use]
pub fn create_icu_collation(collation: &IcuCollation) -> String {
    let tag = collation.as_str();
    format!("CREATE COLLATION IF NOT EXISTS \"{tag}\" (provider = icu, locale = '{tag}')")
}

#[cfg(test)]
mod tests {
    use crate::schema_ddl::icu_collation::create_icu_collation;
    use andromeda_ngsi_ld_query::ordering::collation::IcuCollation;
    use pretty_assertions::assert_eq;

    fn collation(raw: &str) -> IcuCollation {
        IcuCollation::parse(raw).expect("the fixture is a valid ICU collation")
    }

    /// Clause 4.23.3 EXAMPLE 7 names `de-u-co-phonebk`, which PostgreSQL does
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
        for raw in ["und-x-icu", "de-u-co-phonebk", "und-u-ks-identic", "de-DE", "und-u-kn-true"] {
            let rendered = create_icu_collation(&collation(raw));
            assert_eq!(rendered.matches('"').count(), 2, "{rendered}");
            assert_eq!(rendered.matches('\'').count(), 2, "{rendered}");
            assert!(!rendered.contains('\\'), "{rendered}");
        }
    }
}
