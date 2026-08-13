//! What a data source can honestly do.
//!
//! Capabilities are read once at boot and cached in a per-resource feature
//! matrix, a capability is a property of the source, not of the request.
//! The engine visibly degrades to what the source supports and the UI never
//! lies about it.

use crate::query::{FilterOp, SearchMode};

/// Which pagination styles the source can serve.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaginationModes {
    /// Numbered offset pages only.
    Offset,
    /// Opaque cursors only.
    Cursor,
    /// Both; the engine picks per request.
    Both,
}

/// The source's write grade.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WriteCapability {
    /// No writes at all.
    ReadOnly,
    /// Single-record create/update/delete.
    Crud,
    /// Efficient batched mutations (`apply_mutations`).
    Bulk,
}

/// What aggregation the source can back.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AggregationCapability {
    /// No aggregation.
    None,
    /// Measures without grouping.
    Basic,
    /// Measures grouped by a field.
    Grouped,
    /// Date-histogram grouping too.
    Histogram,
}

/// Stale-edit detection the source can enforce.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConcurrencySupport {
    /// No detection; last write wins.
    None,
    /// Version-column style checking.
    Version,
    /// HTTP etag style checking.
    Etag,
}

/// The source's declared abilities.
#[derive(Clone, Debug)]
pub struct Capabilities {
    /// Pagination styles offered.
    pub pagination: PaginationModes,
    /// Cheap filtered counts (page numbers). Absent → prev/next pager.
    pub totals: bool,
    /// Write grade. `ReadOnly` → no create/edit/delete UI at all.
    pub write: WriteCapability,
    /// Real transactions (`begin`). Absent → row-by-row import fallback.
    pub transactions: bool,
    /// What "search" means. `None` → search box not rendered.
    pub search: SearchMode,
    /// Operators the source can express; only these filter UIs are offered.
    pub filter_ops: Vec<FilterOp>,
    /// Any sort at all. Absent → sortable headers not rendered.
    pub sort: bool,
    /// Aggregation grade. `None` → no metric cards for this resource.
    pub aggregation: AggregationCapability,
    /// Stale-edit detection. `None` → UI doesn't promise it.
    pub concurrency: ConcurrencySupport,
    /// Efficient native streaming. Absent → the default page-through pager.
    pub streaming: bool,
    /// Can introspect its own schema (`describe`).
    pub schema_discovery: bool,
}

impl Default for Capabilities {
    /// The conservative baseline: offset pagination, read-only, no search,
    /// no aggregation, no concurrency checking — every upgrade is explicit.
    fn default() -> Self {
        return Self {
            pagination: PaginationModes::Offset,
            totals: false,
            write: WriteCapability::ReadOnly,
            transactions: false,
            search: SearchMode::None,
            filter_ops: Vec::new(),
            sort: false,
            aggregation: AggregationCapability::None,
            concurrency: ConcurrencySupport::None,
            streaming: false,
            schema_discovery: false,
        };
    }
}
