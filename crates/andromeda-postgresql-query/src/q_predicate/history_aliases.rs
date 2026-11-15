//! The names one history probe gives the relations it scans.
//!
//! A probe over a Temporal Evolution reads its attribute source under an alias
//! of its own, and a probe nested inside another takes a suffix so neither
//! shadows the other.

use andromeda_postgresql_schema::temporal::AttributeHistory;
use andromeda_postgresql_stored_form::history_grain::HistoryGrain;
use sea_query::{Alias, Expr as SeaExpr, SimpleExpr};

/// The relations one history probe scans, under the names it gives them.
///
/// A probe nested inside another (a linked-entity hop reading a Relationship's
/// history) takes its own suffix, so its relations cannot shadow the enclosing
/// probe's.
#[derive(Debug, Clone)]
pub(crate) struct HistoryAliases {
    /// Alias of the attribute source scan.
    instance: String,
}

impl HistoryAliases {
    /// The aliases of the top-level probe.
    pub(crate) fn top_level() -> HistoryAliases {
        HistoryAliases { instance: "qh".to_owned() }
    }

    /// The aliases of a linked-entity hop `depth` levels into a traversal.
    pub(crate) fn at_depth(depth: usize) -> HistoryAliases {
        HistoryAliases {
            instance: format!("qh_ln{depth}"),
        }
    }

    /// The attribute source scan.
    pub(crate) fn instance(&self) -> Alias {
        Alias::new(self.instance.as_str())
    }

    /// A column of the instance row this probe scans.
    pub(crate) fn instance_column(&self, column: AttributeHistory) -> SimpleExpr {
        SeaExpr::col((self.instance(), column))
    }

    /// A grain column of the instance row this probe scans.
    pub(crate) fn instance_grain(&self, column: HistoryGrain) -> SimpleExpr {
        SeaExpr::col((self.instance(), column))
    }
}
