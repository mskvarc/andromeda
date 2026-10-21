//! The hash of a sub-attribute's name path: the `attribute_names` keys from
//! the top-level attribute down to the sub-attribute, each folded into the
//! hash of the path above it.

use crate::attribute_name_key::AttributeNameKey;
use xxhash_rust::xxh3::xxh3_64_with_seed;

/// The hash of one name path, the seed every key at that path is salted from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PathHash(u64);

impl PathHash {
    /// The hash of the empty path, above the top-level attribute.
    pub const ROOT: PathHash = PathHash(0);

    /// The hash of the path `names`, the top-level attribute first.
    #[must_use]
    pub fn of(names: &[AttributeNameKey]) -> PathHash {
        names.iter().fold(PathHash::ROOT, |path, name| path.child(*name))
    }

    /// The hash of this path extended by `name`: XXH3-64 of the key's
    /// little-endian bytes, seeded with this path's hash.
    #[must_use]
    pub fn child(self, name: AttributeNameKey) -> PathHash {
        PathHash(xxh3_64_with_seed(&name.get().to_le_bytes(), self.0))
    }

    /// The hash.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use crate::{attribute_name_key::AttributeNameKey, sub_attribute_key::path_hash::PathHash};
    use pretty_assertions::{assert_eq, assert_ne};
    use xxhash_rust::xxh3::xxh3_64_with_seed;

    #[test]
    fn a_path_folds_each_name_into_the_hash_above_it() {
        let by_hand = xxh3_64_with_seed(&7_i32.to_le_bytes(), xxh3_64_with_seed(&2_i32.to_le_bytes(), 0));
        assert_eq!(PathHash::of(&[AttributeNameKey::from(2), AttributeNameKey::from(7)]).get(), by_hand);
    }

    #[test]
    fn the_empty_path_hashes_to_zero() {
        assert_eq!(PathHash::of(&[]), PathHash::ROOT);
        assert_eq!(PathHash::ROOT.get(), 0);
    }

    #[test]
    fn the_order_of_the_names_matters() {
        let forward = PathHash::of(&[AttributeNameKey::from(2), AttributeNameKey::from(7)]);
        let backward = PathHash::of(&[AttributeNameKey::from(7), AttributeNameKey::from(2)]);
        assert_ne!(forward, backward);
    }
}
