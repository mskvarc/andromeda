//! What one update says about a nullable member of a snapshot's metadata.

/// The three outcomes an update can name for one nullable metadata member.
///
/// Clause 5.11 lets a Snapshot update omit a member, set it to `null`, or set
/// it to a value, and the three are distinct: an omitted member keeps whatever
/// is stored, an explicit `null` clears it, and a value replaces it. Nesting one
/// option inside another would spell the first two the same way round the wrong
/// way -- both as an absence -- so the update statement takes this instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MetadataChange<T> {
    /// The update did not name the member; what is stored stands.
    Unchanged,
    /// The update set the member to `null`; what is stored is cleared.
    Cleared,
    /// The update set the member to a value.
    Set(T),
}

impl<T> From<Option<Option<T>>> for MetadataChange<T> {
    fn from(requested: Option<Option<T>>) -> MetadataChange<T> {
        match requested {
            None => MetadataChange::Unchanged,
            Some(None) => MetadataChange::Cleared,
            Some(Some(value)) => MetadataChange::Set(value),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::snapshot::metadata_change::MetadataChange;

    #[test]
    fn an_unnamed_member_is_distinct_from_one_explicitly_set_to_null() {
        let unnamed: MetadataChange<u8> = MetadataChange::from(None);
        let cleared: MetadataChange<u8> = MetadataChange::from(Some(None));
        pretty_assertions::assert_eq!(unnamed, MetadataChange::Unchanged);
        pretty_assertions::assert_eq!(cleared, MetadataChange::Cleared);
        assert_ne!(unnamed, cleared);
    }

    #[test]
    fn a_named_value_is_carried_through_unchanged() {
        pretty_assertions::assert_eq!(MetadataChange::from(Some(Some(7_u8))), MetadataChange::Set(7));
    }
}
