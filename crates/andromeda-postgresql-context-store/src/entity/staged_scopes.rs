//! The Scopes one entity's search row is matched by and read back from.

use andromeda_ngsi_ld_lexicon::scope::path::ScopePath;
use andromeda_postgresql_query::ltree::StoredScopePath;
use sqlx::types::Json;

/// One entity's Scopes as its search row stores them (clause 4.18): the
/// `ltree` label paths a `scopeQ` is matched against and, at the same
/// positions, the Scopes as written, which is what reads back (clause 4.6.4
/// has the broker return the original content, and `Madrid` and `/Madrid`
/// share one label path).
///
/// Each list is bound as the JSON array of its texts, which the entity
/// statements expand into the `scope_paths` `ltree[]` and the
/// `verbatim_scope_paths` `text[]` columns. An entity without a Scope binds
/// SQL NULL for both: no `scopeQ` matches it (clause 4.19), so it costs the
/// Scope index nothing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StagedScopes {
    /// The label paths, in `ltree` dot notation.
    label_paths: Vec<String>,
    /// The Scopes as written.
    written: Vec<String>,
}

impl StagedScopes {
    /// The stored form of `scopes`, in order.
    #[must_use]
    pub fn of(scopes: &[ScopePath]) -> StagedScopes {
        let (label_paths, written) = scopes
            .iter()
            .map(|scope| {
                let stored = StoredScopePath::from(scope);
                (stored.label_path, stored.written.to_owned())
            })
            .unzip();
        StagedScopes { label_paths, written }
    }

    /// The label paths as the `jsonb` a statement binds, `None` for an
    /// entity without a Scope.
    #[must_use]
    pub fn label_paths(&self) -> Option<Json<&[String]>> {
        (!self.label_paths.is_empty()).then_some(Json(&self.label_paths))
    }

    /// The written Scopes as the `jsonb` a statement binds, `None` for an
    /// entity without a Scope.
    #[must_use]
    pub fn written(&self) -> Option<Json<&[String]>> {
        (!self.written.is_empty()).then_some(Json(&self.written))
    }
}

#[cfg(test)]
mod tests {
    use crate::entity::staged_scopes::StagedScopes;
    use andromeda_ngsi_ld_lexicon::scope::path::ScopePath;

    fn scopes(raw: &[&str]) -> Vec<ScopePath> {
        raw.iter().map(|scope| scope.parse().expect("a valid scope")).collect()
    }

    #[test]
    fn each_scope_is_staged_as_its_label_path_and_as_written() {
        let staged = StagedScopes::of(&scopes(&["/root/a", "Madrid"]));
        pretty_assertions::assert_eq!(
            serde_json::to_string(&staged.label_paths().expect("a scoped entity binds its paths").0).expect("serialises"),
            r#"["root.a","Madrid"]"#
        );
        pretty_assertions::assert_eq!(
            serde_json::to_string(&staged.written().expect("a scoped entity binds its scopes").0).expect("serialises"),
            r#"["/root/a","Madrid"]"#
        );
    }

    /// The search row keeps no NULL list beside a non-NULL one and no empty
    /// list, so an entity without a Scope stages two NULLs.
    #[test]
    fn no_scope_is_staged_as_two_nulls() {
        let staged = StagedScopes::of(&[]);
        assert!(staged.label_paths().is_none());
        assert!(staged.written().is_none());
    }
}
