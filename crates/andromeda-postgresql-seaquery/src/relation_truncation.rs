//! Emptying a set of relations in one statement.
//!
//! sea-query truncates one relation and takes no options, so the multi-relation
//! `TRUNCATE ... RESTART IDENTITY CASCADE` is spelled here.

use crate::{
    command::{Command, rendering::RenderedCommand},
    identifier::quoted_tables,
};
use sea_query::TableName;

/// `TRUNCATE <relation>, ... RESTART IDENTITY CASCADE`: every relation emptied
/// together, which keeps foreign keys between them satisfied, and their
/// identity sequences reset.
#[derive(Clone, Debug, PartialEq)]
pub struct RelationTruncation {
    /// The relations emptied.
    relations: Vec<TableName>,
}

impl RelationTruncation {
    /// Empty `relations` together.
    #[must_use]
    pub const fn new(relations: Vec<TableName>) -> RelationTruncation {
        RelationTruncation { relations }
    }
}

impl RenderedCommand for RelationTruncation {
    fn rendered(&self) -> String {
        format!("TRUNCATE {} RESTART IDENTITY CASCADE", quoted_tables(&self.relations))
    }
}

impl Command for RelationTruncation {}

#[cfg(test)]
mod tests {
    use crate::{command::rendering::RenderedCommand, relation_truncation::RelationTruncation};
    use sea_query::{Alias, TableName};

    #[test]
    fn every_relation_is_emptied_in_one_statement_with_its_identities_reset() {
        let relations = vec![
            TableName::from((Alias::new("snap_7_stg"), Alias::new("entities"))),
            TableName::from((Alias::new("snap_7_stg"), Alias::new("entity_search"))),
        ];
        pretty_assertions::assert_eq!(
            RelationTruncation::new(relations).rendered(),
            r#"TRUNCATE "snap_7_stg"."entities", "snap_7_stg"."entity_search" RESTART IDENTITY CASCADE"#
        );
    }
}
