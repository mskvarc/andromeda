//! The `object` a `ListRelationship` is written with once Table 4.3.6.8-1
//! reformats it as a Relationship for a target before 1.8.
//!
//! Table 4.3.6.8-3 has typed a Relationship's `object` "String or String[]"
//! since 1.0, so every target of the list fits, in list order. One target is
//! the single URI, which keeps the deletion encoding of clause 4.5.22.2 (the
//! array of one NGSI-LD Null) the URI clause 4.5.0 makes a Relationship's
//! Null. An empty list names no target and is written as the JSON null.

use andromeda_ngsi_ld_lexicon::uri::Uri;
use serde::{Serialize, Serializer};

/// The targets of a `ListRelationship`, written as a Relationship's `object`.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ListAsRelationship<'a> {
    /// The ordered targets.
    targets: &'a [Uri],
}

impl<'a> ListAsRelationship<'a> {
    /// The `object` `targets` are written as.
    pub(crate) const fn new(targets: &'a [Uri]) -> ListAsRelationship<'a> {
        ListAsRelationship { targets }
    }
}

impl Serialize for ListAsRelationship<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self.targets {
            [] => serializer.serialize_none(),
            [only] => only.serialize(serializer),
            targets => serializer.collect_seq(targets),
        }
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::version::list_as_relationship::ListAsRelationship;
    use andromeda_ngsi_ld_lexicon::uri::Uri;
    use pretty_assertions::assert_eq;
    use serde_json::{Value, json};

    /// The `object` the list of `targets` is written as.
    fn object(targets: &[&str]) -> Value {
        let targets: Vec<Uri> = targets.iter().map(|target| target.parse().expect("a URI")).collect();
        serde_json::to_value(ListAsRelationship::new(&targets)).expect("serialises")
    }

    #[test]
    fn every_target_is_kept_in_list_order() {
        assert_eq!(
            object(&["urn:ngsi-ld:C:1", "urn:ngsi-ld:A:1", "urn:ngsi-ld:B:1"]),
            json!(["urn:ngsi-ld:C:1", "urn:ngsi-ld:A:1", "urn:ngsi-ld:B:1"])
        );
    }

    #[test]
    fn one_target_is_the_single_uri() {
        assert_eq!(object(&["urn:ngsi-ld:A:1"]), json!("urn:ngsi-ld:A:1"));
    }

    /// Clause 4.5.0: a Relationship's NGSI-LD Null is the URI, so the deletion
    /// encoding of clause 4.5.22.2 becomes it.
    #[test]
    fn the_deletion_encoding_is_the_relationship_null() {
        assert_eq!(object(&["urn:ngsi-ld:null"]), json!("urn:ngsi-ld:null"));
    }

    #[test]
    fn an_empty_list_is_the_json_null() {
        assert_eq!(object(&[]), Value::Null);
    }
}
