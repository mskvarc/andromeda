//! Extended statistics over several columns of a table.
//!
//! sea-query has no `CREATE STATISTICS`. The planner assumes columns are
//! independent unless a statistics object tells it otherwise.

use crate::{
    command::{Command, rendering::RenderedCommand},
    identifier::{quoted_iden, quoted_table},
};
use sea_query::{DynIden, IntoIden, TableName};
use strum::Display;

/// One kind of statistics a statistics object collects.
#[derive(Clone, Copy, Debug, Display, Eq, PartialEq)]
#[strum(serialize_all = "lowercase")]
pub enum StatisticsKind {
    /// Functional dependencies between the columns.
    Dependencies,
    /// The number of distinct combinations of the columns.
    Ndistinct,
}

/// `CREATE STATISTICS <name> (<kind>, ...) ON <column>, ... FROM <table>`.
#[derive(Clone, Debug)]
pub struct CreateStatistics {
    /// The statistics object's name.
    name: DynIden,
    /// The kinds collected.
    kinds: Vec<StatisticsKind>,
    /// The columns the statistics span.
    columns: Vec<DynIden>,
    /// The table the columns belong to.
    table: TableName,
}

impl CreateStatistics {
    /// A statistics object `name` collecting `kinds` over `columns` of `table`.
    pub fn new(name: impl IntoIden, kinds: Vec<StatisticsKind>, columns: Vec<DynIden>, table: impl Into<TableName>) -> CreateStatistics {
        CreateStatistics {
            name: name.into_iden(),
            kinds,
            columns,
            table: table.into(),
        }
    }
}

impl RenderedCommand for CreateStatistics {
    fn rendered(&self) -> String {
        let kinds = self.kinds.iter().map(ToString::to_string).collect::<Vec<_>>().join(", ");
        let columns = self.columns.iter().map(quoted_iden).collect::<Vec<_>>().join(", ");
        format!(
            "CREATE STATISTICS {} ({kinds}) ON {columns} FROM {}",
            quoted_iden(&self.name),
            quoted_table(&self.table)
        )
    }
}

impl Command for CreateStatistics {}

#[cfg(test)]
mod tests {
    use crate::{
        command::rendering::RenderedCommand,
        statistics_object::{CreateStatistics, StatisticsKind},
    };
    use sea_query::{Alias, IntoIden, TableName};

    #[test]
    fn the_object_lists_its_kinds_and_the_columns_it_spans() {
        pretty_assertions::assert_eq!(
            CreateStatistics::new(
                Alias::new("fixture_stx"),
                vec![StatisticsKind::Dependencies, StatisticsKind::Ndistinct],
                vec![Alias::new("a").into_iden(), Alias::new("b").into_iden()],
                TableName::from(Alias::new("fixture"))
            )
            .rendered(),
            r#"CREATE STATISTICS "fixture_stx" (dependencies, ndistinct) ON "a", "b" FROM "fixture""#
        );
    }
}
