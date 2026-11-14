//! The stored form of a clause 4.18 Scope.
//!
//! A stored Scope is its `ltree` label path, which a `scopeQ` is matched
//! against, beside the text it was written in, which is the only form read
//! back. A label path forgets whether the Scope was written with its
//! optional leading `/`, so nothing rebuilds a Scope from one.

use andromeda_ngsi_ld_lexicon::scope::path::ScopePath;

/// The two columns one stored clause 4.18 Scope occupies.
///
/// A store keeps both, side by side: the label path in an `ltree` column that
/// `scopeQ` predicates and their index read, and the written text in a text
/// column that the read path parses back with `ScopePath::from_str`. Clause
/// 4.6.4 has the broker "return the original content", and only the text
/// holds it, since `Madrid` and `/Madrid` share the label path `Madrid`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoredScopePath<'a> {
    /// The `ltree` label path: the levels joined by `.`, so `/A/B/C` is
    /// `A.B.C` and `Madrid` is `Madrid`.
    pub label_path: String,
    /// The Scope as it was written.
    pub written: &'a str,
}

impl<'a> From<&'a ScopePath> for StoredScopePath<'a> {
    fn from(scope: &'a ScopePath) -> StoredScopePath<'a> {
        StoredScopePath {
            label_path: label_path(scope),
            written: scope.as_str(),
        }
    }
}

/// The `ltree` label path of a scope path.
///
/// Total by construction: a [`ScopePath`] is a hierarchical clause 4.18 path
/// whose levels satisfy the same label grammar `ltree` accepts and stay within
/// the label length it can hold, and the NGSI-LD Null of a deleted scope is not
/// one of its values.
fn label_path(scope: &ScopePath) -> String {
    let mut ltree = String::with_capacity(scope.as_str().len());
    for (index, level) in scope.levels().enumerate() {
        if index > 0 {
            ltree.push('.');
        }
        ltree.push_str(level);
    }
    ltree
}

#[cfg(test)]
mod tests {
    use crate::stored_scope_path::StoredScopePath;
    use andromeda_ngsi_ld_lexicon::scope::path::ScopePath;
    use pretty_assertions::assert_eq;

    fn path(raw: &str) -> ScopePath {
        raw.parse::<ScopePath>().expect("valid scope path")
    }

    /// A stored Scope keeps the text it was written in beside the label path
    /// it is matched by.
    #[test]
    fn a_stored_scope_keeps_its_written_text_beside_its_label_path() {
        let bare = path("Madrid");
        let nested = path("/A/B/C");
        assert_eq!(
            StoredScopePath::from(&bare),
            StoredScopePath {
                label_path: "Madrid".to_owned(),
                written: "Madrid"
            }
        );
        assert_eq!(
            StoredScopePath::from(&nested),
            StoredScopePath {
                label_path: "A.B.C".to_owned(),
                written: "/A/B/C"
            }
        );
    }

    /// Both spellings name one place, so a `scopeQ` matches them alike.
    #[test]
    fn both_spellings_share_one_label_path() {
        let bare = path("Madrid");
        let rooted = path("/Madrid");
        assert_eq!(StoredScopePath::from(&bare).label_path, StoredScopePath::from(&rooted).label_path);
    }

    #[test]
    fn unicode_levels_survive_the_label_path() {
        let unicode = path("/M\u{00fc}nchen/Stra\u{00df}e");
        assert_eq!(StoredScopePath::from(&unicode).label_path, "M\u{00fc}nchen.Stra\u{00df}e");
    }
}
