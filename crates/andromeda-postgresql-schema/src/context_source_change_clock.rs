//! The `context_source_change_clock` relation: the one counter every write of
//! a context source registration and every client write of a context source
//! subscription advances, so their revisions order them as they committed.

use sea_query::Iden;

/// The `context_source_change_clock` table - a singleton counter.
#[derive(Iden, Clone, Copy)]
pub enum ContextSourceChangeClock {
    /// Table name: `context_source_change_clock`
    Table,
    /// The revision the latest write drew
    Revision,
}

#[cfg(test)]
mod tests {
    use crate::context_source_change_clock::ContextSourceChangeClock;
    use pretty_assertions::assert_eq;
    use sea_query::Iden;

    #[test]
    fn context_source_change_clock_table_and_columns() {
        assert_eq!(ContextSourceChangeClock::Table.to_string(), "context_source_change_clock");
        assert_eq!(ContextSourceChangeClock::Revision.to_string(), "revision");
    }
}
