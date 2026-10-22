//! The names one history probe gives the relations it scans.
//!
//! A probe over a Temporal Evolution reads its stream source, its attribute
//! source and the parts of a stream it places the instance in effect when a
//! window opens from, each under an alias of its own, and a probe nested inside another
//! takes a suffix so neither shadows the other.

use crate::history_source::grain::HistoryGrain;
use andromeda_postgresql_schema::temporal::AttributeHistory;
use sea_query::{Alias, Expr as SeaExpr, SimpleExpr};

/// The relations one history probe scans, under the names it gives them.
///
/// A probe nested inside another (a linked-entity hop reading a Relationship's
/// history) takes its own suffix, so its relations cannot shadow the enclosing
/// probe's.
#[derive(Debug, Clone)]
pub(crate) struct HistoryAliases {
    /// Alias of the stream source scan.
    stream: String,
    /// Alias of the attribute source scan.
    instance: String,
    /// Alias of a read limited to the rows at the latest placement.
    placement: String,
    /// Alias of one part of a stream the instance in effect is placed in.
    placed_instance: String,
}

impl HistoryAliases {
    /// The aliases of the top-level probe.
    pub(crate) fn top_level() -> HistoryAliases {
        HistoryAliases {
            stream: "qs".to_owned(),
            instance: "qh".to_owned(),
            placement: "qe".to_owned(),
            placed_instance: "qi".to_owned(),
        }
    }

    /// The aliases of a linked-entity hop `depth` levels into a traversal.
    pub(crate) fn at_depth(depth: usize) -> HistoryAliases {
        HistoryAliases {
            stream: format!("qs_ln{depth}"),
            instance: format!("qh_ln{depth}"),
            placement: format!("qe_ln{depth}"),
            placed_instance: format!("qi_ln{depth}"),
        }
    }

    /// The stream source scan.
    pub(crate) fn stream(&self) -> Alias {
        Alias::new(self.stream.as_str())
    }

    /// The attribute source scan.
    pub(crate) fn instance(&self) -> Alias {
        Alias::new(self.instance.as_str())
    }

    /// One part of a stream the instance in effect is placed in.
    pub(crate) fn placed_instance(&self) -> Alias {
        Alias::new(self.placed_instance.as_str())
    }

    /// A read limited to the rows at the latest placement.
    pub(crate) fn placement(&self) -> Alias {
        Alias::new(self.placement.as_str())
    }

    /// A column of the instance row this probe scans.
    pub(crate) fn instance_column(&self, column: AttributeHistory) -> SimpleExpr {
        SeaExpr::col((self.instance(), column))
    }

    /// A grain column of the instance row this probe scans.
    pub(crate) fn instance_grain(&self, column: HistoryGrain) -> SimpleExpr {
        SeaExpr::col((self.instance(), column))
    }

    /// A grain column of the stream row this probe scans.
    pub(crate) fn stream_column(&self, column: HistoryGrain) -> SimpleExpr {
        SeaExpr::col((self.stream(), column))
    }
}
