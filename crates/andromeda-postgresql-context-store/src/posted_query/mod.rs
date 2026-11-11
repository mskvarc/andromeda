//! Persistence of posted queries: the paged queries sent by POST that the
//! broker holds so the page links of their result can name them (clauses
//! 5.5.9 and 6.3.10).

/// The number a posted query's API is stored as.
pub mod kind_code;
/// The statements over `posted_queries` rows, and the lock every write takes.
pub mod lifecycle;
/// The columns a posted query is written as, and what a live row reads back as.
pub mod mapper;
/// Keeping a tenant's posted queries within its quota.
pub mod quota;
/// The posted-query repository struct.
pub mod repo;
/// The `PostedQueryStore` implementation.
pub mod store;
