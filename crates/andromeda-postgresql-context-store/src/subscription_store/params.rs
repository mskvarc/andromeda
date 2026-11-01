//! Column-shape facts the `subscriptions` table's generated statements need.

/// Whether the subscription table includes a `dataset_ids` column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DatasetIdPresence {
    /// The `dataset_ids` column exists and should be read.
    Present,
    /// The `dataset_ids` column does not exist in this table.
    Absent,
}
