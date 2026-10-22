//! Handing a utility command to the driver.
//!
//! A utility command (schema and database DDL, a setting, maintenance) takes
//! no bound value: PostgreSQL reads its identifiers and literals from the text
//! alone, which this crate quotes. It runs on the simple protocol, which
//! carries a command of several statements in one round trip and leaves no
//! prepared statement behind on the connection. Like [`crate::statement`],
//! the rendering is private to this crate.

use sqlx::{AssertSqlSafe, RawSql};

/// The rendering every command of this crate shares, out of reach of other
/// crates so no command text leaves this one.
pub(crate) mod rendering {
    /// A command's text.
    pub trait RenderedCommand {
        /// The text of the command.
        fn rendered(&self) -> String;
    }
}

/// A utility command, run on the simple protocol without bound values.
pub trait Command: rendering::RenderedCommand + Send + Sync {
    /// The driver query running the command.
    fn to_query(&self) -> RawSql {
        sqlx::raw_sql(AssertSqlSafe(self.rendered()))
    }
}
