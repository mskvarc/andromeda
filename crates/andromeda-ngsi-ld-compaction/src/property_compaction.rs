//! Compacting the items of one expanded property into the object that holds
//! them (JSON-LD 1.1 API section 6.1, the expanded-property step).
//!
//! Each item selects its own key (a language-tagged string and a node
//! reference of one property can land under different terms), and is added
//! under it by the selected term's container: a `@list` term takes a list's
//! items as its array, a `@language` term keys strings by their language, and
//! every other term takes the compacted item through the add-value rule.

use crate::{
    compacted::{Compacted, CompactedObject},
    compacted_mode::CompactedMode,
    expanded_item::{ExpandedItem, scalar_of},
    member_insertion::{add_value, add_whole_value},
    undecidable::Undecidable,
    walk::Walk,
};
use jsonld::{
    BlankIdBuf,
    Container,
    ContainerKind,
    IriBuf,
    Term,
    Value,
    compaction::{ValueHint, ValueKind},
    syntax::Keyword,
};

impl Walk<'_> {
    /// Compact every item of `property` into `result`.
    ///
    /// A property holding no item still gets its key, with an empty array.
    pub(crate) fn compact_property<'a, M: CompactedMode>(
        &mut self,
        result: &mut CompactedObject<'a>,
        property: &Term<IriBuf, BlankIdBuf>,
        items: &[ExpandedItem<'a, M>],
    ) -> Result<(), Undecidable> {
        if items.is_empty() {
            if let Some(key) = self.terms.property_key(property, ValueHint::new(ValueKind::EmptyArray))? {
                add_value(result, &key, Compacted::Array(Vec::new()), true);
            }
            return Ok(());
        }
        for item in items {
            let key = match item {
                ExpandedItem::List(list) => {
                    let kinds: Vec<ValueKind<'_, IriBuf, BlankIdBuf>> = list.iter().map(ExpandedItem::kind).collect();
                    self.terms.property_key(property, ValueHint::new(ValueKind::List(&kinds)))?
                }
                ExpandedItem::Value(_)
                | ExpandedItem::Reference(_)
                | ExpandedItem::Node(..)
                | ExpandedItem::StoredInstance(_)
                | ExpandedItem::WrittenInstance(_)
                | ExpandedItem::Geometry(_)
                | ExpandedItem::WrappedObject(_) => self.terms.property_key(property, ValueHint::new(item.kind()))?,
            };
            if let Some(key) = key {
                self.place(result, &key, item)?;
            }
        }
        Ok(())
    }

    /// Compact the instance nodes of one attribute into `result`.
    ///
    /// Every instance node selects the same key, so it is selected once, and
    /// remembered for the request by the attribute's name.
    pub(crate) fn compact_attribute<'a, M: CompactedMode>(
        &mut self,
        result: &mut CompactedObject<'a>,
        attribute: &Term<IriBuf, BlankIdBuf>,
        instances: &[ExpandedItem<'a, M>],
    ) -> Result<(), Undecidable> {
        let Some(key) = self.terms.attribute_key(attribute)? else {
            return Ok(());
        };
        if instances.is_empty() {
            add_value(result, &key, Compacted::Array(Vec::new()), true);
        }
        instances.iter().try_for_each(|instance| self.place(result, &key, instance))
    }

    /// Add one compacted item under `key`, as the key's container says.
    fn place<'a, M: CompactedMode>(&mut self, result: &mut CompactedObject<'a>, key: &str, item: &ExpandedItem<'a, M>) -> Result<(), Undecidable> {
        let container = self.terms.container(key);
        let as_array = self.terms.values_as_array(key);
        if let ExpandedItem::List(items) = item {
            return self.place_list(result, key, items, container, as_array);
        }
        let compacted = self.compact_item(item, key)?;
        if !container.contains(ContainerKind::Language) {
            if is_json_literal(item) {
                add_whole_value(result, key, compacted, as_array);
            } else {
                add_value(result, key, compacted, as_array);
            }
            return Ok(());
        }
        // Under a `@language` container a value object is written as its bare
        // string under its language, and anything else under `@none`.
        let (compacted, language) = match item {
            ExpandedItem::Value(value) => (scalar_of(value), value.language().map(ToString::to_string)),
            ExpandedItem::Reference(_)
            | ExpandedItem::Node(..)
            | ExpandedItem::List(_)
            | ExpandedItem::StoredInstance(_)
            | ExpandedItem::WrittenInstance(_)
            | ExpandedItem::Geometry(_)
            | ExpandedItem::WrappedObject(_) => (compacted, None),
        };
        let map_key = language.unwrap_or_else(|| self.terms.keyword(Keyword::None).to_owned());
        if result.get(key).is_none() {
            result.insert(key, Compacted::Object(CompactedObject::new()));
        }
        let map = result.get_mut(key).and_then(Compacted::as_object_mut).ok_or(Undecidable::CollidingEntry)?;
        if is_json_literal(item) {
            add_whole_value(map, &map_key, compacted, as_array);
        } else {
            add_value(map, &map_key, compacted, as_array);
        }
        Ok(())
    }

    /// Add a list object under `key`: as the key's array under a `@list`
    /// term, as a `{"@list": [..]}` object otherwise.
    fn place_list<'a, M: CompactedMode>(
        &mut self,
        result: &mut CompactedObject<'a>,
        key: &str,
        items: &[ExpandedItem<'a, M>],
        container: Container,
        as_array: bool,
    ) -> Result<(), Undecidable> {
        let compacted = match self.compact_collection(items, key)? {
            array @ Compacted::Array(_) => array,
            single
            @ (Compacted::Null | Compacted::Boolean(_) | Compacted::Number(_) | Compacted::String(_) | Compacted::Object(_) | Compacted::Geometry(_)) => {
                Compacted::Array(vec![single])
            }
        };
        if container.contains(ContainerKind::List) {
            result.insert(key, compacted);
        } else {
            let mut list = CompactedObject::new();
            list.insert(self.terms.keyword(Keyword::List), compacted);
            add_value(result, key, Compacted::Object(list), as_array);
        }
        Ok(())
    }

    /// Compact the items of a list under `key`, dropping an item that
    /// compacts to `null`, and write a single remaining item bare unless the
    /// key keeps an array.
    pub(crate) fn compact_collection<'a, M: CompactedMode>(&mut self, items: &[ExpandedItem<'a, M>], key: &str) -> Result<Compacted<'a>, Undecidable> {
        let mut compacted = Vec::with_capacity(items.len());
        for item in items {
            let item = self.compact_item(item, key)?;
            if !item.is_null() {
                compacted.push(item);
            }
        }
        let container = self.terms.container(key);
        let keeps_array = container.contains(ContainerKind::List)
            || container.contains(ContainerKind::Set)
            || key == Keyword::Graph.into_str()
            || key == Keyword::Set.into_str();
        if compacted.len() != 1 || !self.terms.compacts_arrays() || keeps_array {
            return Ok(Compacted::Array(compacted));
        }
        Ok(compacted.into_iter().next().unwrap_or(Compacted::Null))
    }

    /// Compact a list held inside another list: the inner list's items as an
    /// array under a `@list` key, a list object otherwise.
    pub(crate) fn compact_nested_list<'a, M: CompactedMode>(&mut self, items: &[ExpandedItem<'a, M>], key: &str) -> Result<Compacted<'a>, Undecidable> {
        if self.terms.container(key).contains(ContainerKind::List) {
            return self.compact_collection(items, key);
        }
        let mut result = CompactedObject::new();
        self.compact_property(&mut result, &Term::Keyword(Keyword::List), items)?;
        Ok(Compacted::Object(result))
    }
}

/// Whether `item` is a `@json` literal, whose compacted document is added
/// whole.
const fn is_json_literal<M: CompactedMode>(item: &ExpandedItem<'_, M>) -> bool {
    matches!(item, ExpandedItem::Value(Value::Json(_)))
}
