//! Internationalization support for NGSI-LD (Annex G).
//!
//! Provides collation metadata augmentation (G.2), locale-insensitive
//! normalization, query rewriting for collation-aware queries, and
//! locale validation (BCP 47).

pub mod collation;
pub mod validation;
