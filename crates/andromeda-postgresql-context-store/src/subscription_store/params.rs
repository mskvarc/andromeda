//! Column-shape facts the `subscriptions` table's generated statements need.

/// Whether the subscription table includes a `dataset_id` column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DatasetIdPresence {
    /// The `dataset_id` column exists and should be read.
    Present,
    /// The `dataset_id` column does not exist in this table.
    Absent,
}
