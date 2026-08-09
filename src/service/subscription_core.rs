//! Shared subscription CRUD logic used by both entity subscriptions and
//! context-source subscriptions.

use crate::error::AndromedaError;
use andromeda_ngsi_ld_query::filter::expression::Expr;
use chrono::Utc;
use serde_json::Value;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Subscription persistence field extraction and update merging.
pub mod fields;
/// Subscription persistence normalization.
pub mod normalization;
/// Subscription payload validation.
pub mod validation;
