//! Error types for index operations.

/// Errors that can occur during index operations.
///
/// # Variants
///
/// * `InvalidSearchOptions` — The provided search options are invalid.
/// * `EntryIdExhausted` — No more entry IDs can be allocated.
/// * `InternalInvariantViolation` — An internal consistency check failed.
#[allow(dead_code)]
#[derive(Debug, thiserror::Error)]
pub enum IndexError {
    /// The provided search options are invalid.
    #[error("invalid search options")]
    InvalidSearchOptions,
    /// The entry ID space is exhausted.
    #[error("entry ID space exhausted")]
    EntryIdExhausted,
    /// An internal invariant was violated.
    #[error("internal invariant violated")]
    InternalInvariantViolation,
}
