//! Session-level `SET` statements.
//!
//! `SET` alters a connection's own configuration for the rest of its life and
//! takes no bound parameters, so it is not a SeaQuery statement; the setting
//! value is a configuration literal chosen from a fixed vocabulary, not
//! caller-supplied data.

/// The `SET synchronous_commit = '<setting>'` statement.
///
/// The setting value comes from the broker's own configuration enum
/// (`SynchronousCommit::as_postgres_setting`), one of a fixed set of PostgreSQL
/// tokens (`on`, `off`, `local`, `remote_write`, `remote_apply`), so it is a
/// configuration literal rather than external input.
pub fn set_synchronous_commit(setting: &str) -> String {
    format!("SET synchronous_commit = '{setting}'")
}

#[cfg(test)]
mod tests {
    use crate::schema_ddl::session::set_synchronous_commit;

    #[test]
    fn the_setting_value_is_quoted() {
        assert_eq!(set_synchronous_commit("local"), "SET synchronous_commit = 'local'");
    }
}
