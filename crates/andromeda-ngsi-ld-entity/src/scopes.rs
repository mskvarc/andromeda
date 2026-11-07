//! The scopes one entity belongs to (clause 4.18).
//!
//! Clause 4.18 lets an entity be "assigned to one or more Scopes at the same
//! time" and makes the member optional, so an entity either has no scope at all
//! or at least one path. The NGSI-LD Null is a deletion instruction, never a
//! scope, and [`ScopePath`] already refuses it.

use crate::{
    distinct_paths::DistinctPaths,
    non_empty::{EmptyCollection, NonEmpty},
};
use andromeda_ngsi_ld_lexicon::scope::path::ScopePath;
use derive_more::{AsRef, Deref};

/// One or more scope paths, in order, each at most once.
#[derive(AsRef, Clone, Debug, Deref, Eq, PartialEq)]
#[as_ref(forward)]
pub struct Scopes(NonEmpty<ScopePath>);

impl Scopes {
    /// Hold exactly one path.
    #[must_use]
    pub fn one(path: ScopePath) -> Scopes {
        Scopes(NonEmpty::one(path))
    }

    /// Hold `paths` in order, keeping the first occurrence of a repeated path.
    /// Repeats are found by hash, so a member of millions of paths is read in
    /// linear time.
    ///
    /// # Errors
    ///
    /// Returns [`EmptyCollection`] when `paths` is empty.
    pub fn try_from_paths(paths: impl IntoIterator<Item = ScopePath>) -> Result<Scopes, EmptyCollection> {
        Scopes::try_from_distinct(paths.into_iter().collect())
    }

    /// Hold the paths of `distinct`, in the order they were first inserted. A
    /// reader that de-duplicates as it reads hands its paths over here, so no
    /// second copy of them is built to find the repeats.
    ///
    /// # Errors
    ///
    /// Returns [`EmptyCollection`] when `distinct` is empty.
    pub fn try_from_distinct(distinct: DistinctPaths) -> Result<Scopes, EmptyCollection> {
        NonEmpty::try_from_vec(distinct.into_vec()).map(Scopes)
    }

    /// Add the paths this set lacks, in the order given (the clause 5.6.3.4
    /// `noOverwrite` scope append).
    pub fn extend(&mut self, paths: impl IntoIterator<Item = ScopePath>) {
        for path in paths {
            if !self.0.contains(&path) {
                self.0.push(path);
            }
        }
    }

    /// Fold another set into this one, keeping each spelling once: the clause
    /// 4.18 merge of two representations of one Entity, whose "values of
    /// scope are merged". Values are compared as written, so `Madrid` and
    /// `/Madrid` both survive, as two Scopes naming one place.
    pub fn merge(&mut self, other: Scopes) {
        self.extend(other.0.into_vec());
    }

    /// The paths as a slice, in order.
    #[must_use]
    pub fn as_slice(&self) -> &[ScopePath] {
        self.0.as_slice()
    }
}

#[cfg(test)]
mod tests {
    use crate::{non_empty::EmptyCollection, scopes::Scopes};
    use andromeda_ngsi_ld_lexicon::scope::path::ScopePath;

    fn path(raw: &str) -> ScopePath {
        raw.parse().expect("a valid scope path")
    }

    #[test]
    fn a_repeated_path_is_kept_once() {
        let scopes = Scopes::try_from_paths([path("/a"), path("/b"), path("/a")]).expect("two paths");
        pretty_assertions::assert_eq!(scopes.as_slice(), &[path("/a"), path("/b")]);
    }

    #[test]
    fn many_distinct_paths_are_held_in_linear_time() {
        let paths = (0..200_000).map(|number| path(&format!("/s{number}")));
        pretty_assertions::assert_eq!(Scopes::try_from_paths(paths).expect("many paths").as_slice().len(), 200_000);
    }

    #[test]
    fn merging_appends_only_the_paths_held_nowhere_yet() {
        let mut held = Scopes::try_from_paths([path("/a"), path("b")]).expect("two paths");
        held.merge(Scopes::try_from_paths([path("b"), path("/b"), path("c")]).expect("three paths"));
        pretty_assertions::assert_eq!(held.as_slice(), &[path("/a"), path("b"), path("/b"), path("c")]);
    }

    #[test]
    fn no_path_is_refused() {
        pretty_assertions::assert_eq!(Scopes::try_from_paths([]), Err(EmptyCollection));
    }
}
