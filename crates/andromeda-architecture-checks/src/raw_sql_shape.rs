//! The shapes of hand-written SQL the raw-SQL guard reports.

use proc_macro2::Ident;
use thiserror::Error;

/// One syntactic shape that builds SQL by hand, carrying the name it reports.
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
    #[error("{function} spells a raw SQL fragment")]
    CustomFragment {
        /// The called function's name.
        function: Ident,
    },
    /// A `query*` function called with a raw SQL string.
    #[error("{function} is built from a raw SQL string")]
    RawStringFunction {
        /// The called function's name.
        function: Ident,
    },
    /// A `query*` method called with a raw SQL string.
    #[error("{method} is built from a raw SQL string")]
    RawStringMethod {
        /// The called method's name.
        method: Ident,
    },
    /// A formatting macro whose template reads as SQL.
    #[error("{name}! assembles a SQL string")]
    AssembledString {
        /// The macro's name without the bang.
        name: Ident,
    },
}
