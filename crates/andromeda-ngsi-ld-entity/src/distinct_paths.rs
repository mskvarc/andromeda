//! Scope paths in the order they were first met, each held once.
//!
//! A `scope` member can name millions of paths, so what holding them costs per
//! path is what bounds the broker's memory while the member is read. The
//! paths live in one vector and the repeats are found through a table of
//! positions into it, which costs one machine word per path where a set that
//! owns its keys costs the path again.

use andromeda_ngsi_ld_lexicon::scope::path::ScopePath;
use hashbrown::HashTable;
use std::{collections::hash_map::RandomState, hash::BuildHasher};

/// Scope paths in first-met order, none twice.
#[derive(Debug, Default)]
pub struct DistinctPaths {
    /// The paths, each once, in the order they were first inserted.
    paths: Vec<ScopePath>,
    /// The position in `paths` of every path it holds, found by the path's
    /// hash.
    positions: HashTable<usize>,
    /// Hashes the paths; seeded per instance so that a request cannot choose
    /// paths that collide.
    hasher: RandomState,
}

impl DistinctPaths {
    /// Hold `path` unless it is held already.
    pub fn insert(&mut self, path: ScopePath) {
        let DistinctPaths { paths, positions, hasher } = self;
        let hash = hasher.hash_one(&path);
        if positions.find(hash, |&position| paths.get(position) == Some(&path)).is_some() {
            return;
        }
        positions.insert_unique(hash, paths.len(), |&position| paths.get(position).map_or(0, |held| hasher.hash_one(held)));
        paths.push(path);
    }

    /// Whether no path is held.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.paths.is_empty()
    }

    /// Give the paths up, in first-met order.
    #[must_use]
    pub fn into_vec(self) -> Vec<ScopePath> {
        self.paths
    }
}

impl FromIterator<ScopePath> for DistinctPaths {
    fn from_iter<I: IntoIterator<Item = ScopePath>>(paths: I) -> DistinctPaths {
        let mut distinct = DistinctPaths::default();
        for path in paths {
            distinct.insert(path);
        }
        distinct
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::distinct_paths::DistinctPaths;
    use andromeda_ngsi_ld_lexicon::scope::path::ScopePath;

    fn path(raw: &str) -> ScopePath {
        raw.parse().expect("a valid scope path")
    }

    #[test]
    fn a_repeated_path_is_held_once_at_its_first_position() {
        let held: DistinctPaths = [path("/a"), path("/b"), path("/a"), path("/c"), path("/b")].into_iter().collect();
        pretty_assertions::assert_eq!(held.into_vec(), vec![path("/a"), path("/b"), path("/c")]);
    }

    #[test]
    fn two_spellings_of_one_place_are_two_paths() {
        let held: DistinctPaths = [path("a"), path("/a")].into_iter().collect();
        pretty_assertions::assert_eq!(held.into_vec(), vec![path("a"), path("/a")]);
    }

    #[test]
    fn many_distinct_paths_survive_the_table_growing() {
        let held: DistinctPaths = (0..100_000)
            .flat_map(|number| [path(&format!("/s{number}")), path(&format!("/s{}", number / 2))])
            .collect();
        pretty_assertions::assert_eq!(held.into_vec().len(), 100_000);
    }

    #[test]
    fn nothing_inserted_is_empty() {
        assert!(DistinctPaths::default().is_empty());
    }
}
