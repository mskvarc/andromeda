//! Which pointer family a fresh request is paged by.

use serde::{Deserialize, Serialize};
use strum::{Display, EnumString};

/// The pointers a request that names neither `offset` nor `cursor` is answered
/// with (`[pagination] default_mode`).
///
/// Both are clause 5.5.9 mechanisms: the opaque pointer of clause 5.5.9.1 and
/// the transparent `limit`/`offset` pair of clause 5.5.9.2. A request that
/// names one of them is paged by it whatever the default is.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize, Display, EnumString)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case")]
pub enum PagingMode {
    /// Walk the collection by opaque, keyset-backed cursors (clause 5.5.9.1).
    #[default]
    Cursor,
    /// Page the collection by `limit`/`offset` (clause 5.5.9.2).
    Offset,
}
