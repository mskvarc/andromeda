//! The shapes of hand-written SQL the raw-SQL guard reports.

use proc_macro2::Ident;
use thiserror::Error;

/// One syntactic shape that spells SQL or hands it to the driver outside the
/// adapter, carrying the name it reports.
///
/// Ordering and equality follow the variant then the name, which keeps the
/// sorted, deduplicated site list stable.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Error)]
pub enum RawSqlShape {
    /// A `&str` constant whose body reads as SQL.
    #[error("constant {name} holds SQL text")]
    Constant {
        /// The constant's identifier.
        name: Ident,
    },
    /// A `&str` static whose body reads as SQL.
    #[error("static {name} holds SQL text")]
    Static {
        /// The static's identifier.
        name: Ident,
    },
    /// A sea-query escape hatch such as `Func::cust`.
    #[error("{name} spells a raw SQL fragment")]
    CustomFragment {
        /// The escape hatch's name.
        name: Ident,
    },
    /// A driver item that takes statement text: a `sqlx` statement
    /// constructor, `AssertSqlSafe`, `sqlx`'s own query builder, or the
    /// `sea-query-sqlx` binder.
    #[error("{name} hands statement text to the driver")]
    DriverText {
        /// The item's name.
        name: Ident,
    },
    /// A method that takes or produces statement text, or an executor method
    /// given a string.
    #[error("{method} runs statement text")]
    TextMethod {
        /// The method's name.
        method: Ident,
    },
    /// A formatting macro whose template reads as SQL.
    #[error("{name}! assembles a SQL string")]
    AssembledString {
        /// The macro's name without the bang.
        name: Ident,
    },
    /// Production code rendering a statement to text.
    #[error("{name} renders a statement to text")]
    Rendering {
        /// The builder's name.
        name: Ident,
    },
}
