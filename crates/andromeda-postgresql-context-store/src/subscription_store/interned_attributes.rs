//! A subscription's attribute lists, as the `attribute_names` keys they are
//! stored as.
//!
//! `watchedAttributes`, `expandValues` and `jsonKeys` (Table 5.2.12-1) are lists
//! of expanded attribute IRIs, stored as `INTEGER[]` of the tenant-global
//! dictionary keys every other relation interns attribute names by. The keys
//! are minted on the connection of the subscription's own write transaction, so
//! the dictionary rows and the subscription that references them commit or roll
//! back together. Each list keeps the order the subscriber wrote it in; the read
//! projection resolves the keys back in that order.

use crate::subscription_store::failure::persistence_error;
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_postgresql_name_dictionary::name_lookup::NameLookup;
use andromeda_postgresql_query::attribute_name_key::AttributeNameKey;
use andromeda_subscription::{error::SubscriptionError, store::SubscriptionFields, subscription_invariant::SubscriptionInvariant};
use sqlx::PgConnection;
use std::collections::HashMap;

/// The dictionary keys of a subscription's three attribute lists, each in the
/// order the subscriber wrote it and absent where the list is.
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct InternedAttributeLists {
    /// The keys of `watchedAttributes`.
    pub(super) watched_attributes: Option<Vec<i32>>,
    /// The keys of `expandValues`.
    pub(super) expand_values: Option<Vec<i32>>,
    /// The keys of `jsonKeys`.
    pub(super) json_keys: Option<Vec<i32>>,
}

/// Resolve every name the three lists carry on `connection`, minting the new
/// ones, and return the lists as keys.
///
/// A dictionary past its bound refuses the write through the same
/// persistence path every other interning write takes.
pub(super) async fn intern_attribute_lists(
    attribute_names: &dyn NameLookup<AttributeName>,
    connection: &mut PgConnection,
    fields: &SubscriptionFields<'_>,
) -> Result<InternedAttributeLists, SubscriptionError> {
    let lists = [fields.watched_attributes, fields.expand_values, fields.json_keys];
    // clone: the dictionary port takes one owned batch, and the three lists are
    // separate borrowed slices of the caller's fields
    let names: Vec<AttributeName> = lists.iter().flatten().flat_map(|list| list.iter().cloned()).collect();
    if names.is_empty() {
        return Ok(InternedAttributeLists::default());
    }
    let keys = attribute_names
        .resolve_or_create(connection, &names)
        .await
        .map_err(|error| persistence_error(error))?;
    Ok(InternedAttributeLists {
        watched_attributes: keys_of(fields.watched_attributes, &keys)?,
        expand_values: keys_of(fields.expand_values, &keys)?,
        json_keys: keys_of(fields.json_keys, &keys)?,
    })
}

/// One list as keys, in its own order.
fn keys_of(list: Option<&[AttributeName]>, keys: &HashMap<AttributeName, AttributeNameKey>) -> Result<Option<Vec<i32>>, SubscriptionError> {
    list.map(|names| {
        names
            .iter()
            .map(|name| {
                keys.get(name)
                    .map(|key| key.get())
                    .ok_or_else(|| SubscriptionError::from(SubscriptionInvariant::AttributeNotInterned))
            })
            .collect()
    })
    .transpose()
}

#[cfg(test)]
mod tests {
    use crate::subscription_store::interned_attributes::keys_of;
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use andromeda_postgresql_query::attribute_name_key::AttributeNameKey;
    use pretty_assertions::assert_eq;
    use std::collections::HashMap;

    fn name(raw: &str) -> AttributeName {
        raw.parse().expect("a valid attribute name")
    }

    #[test]
    fn a_list_keeps_the_order_it_was_written_in() {
        let keys = HashMap::from([
            (name("https://example.org/b"), AttributeNameKey::from(2)),
            (name("https://example.org/a"), AttributeNameKey::from(7)),
        ]);
        let list = [name("https://example.org/b"), name("https://example.org/a")];
        assert_eq!(keys_of(Some(&list), &keys).expect("every name is interned"), Some(vec![2, 7]));
    }

    #[test]
    fn an_absent_list_stays_absent() {
        assert_eq!(keys_of(None, &HashMap::new()).expect("nothing to resolve"), None);
    }

    #[test]
    fn a_name_the_dictionary_did_not_return_is_an_invariant_failure() {
        let list = [name("https://example.org/missing")];
        assert!(keys_of(Some(&list), &HashMap::new()).is_err());
    }
}
