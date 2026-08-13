//! The bounded query model every data source translates.
//!
//! One struct family with source-independent meaning: a filter tree, sort,
//! search, pagination, and projection. Adapters map these to their native
//! dialect (SQL `WHERE`, ES `bool` queries, GraphQL `OR`, …); the framework
//! never speaks anything richer.

use chrono::{DateTime, Utc};

/// A filter operator with source-independent meaning.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FilterOp {
    /// Field equals the value.
    Eq,
    /// Field does not equal the value.
    Ne,
    /// Field is greater than the value.
    Gt,
    /// Field is greater than or equal to the value.
    Gte,
    /// Field is less than the value.
    Lt,
    /// Field is less than or equal to the value.
    Lte,
    /// Field is a member of the value list.
    In,
    /// Field is not a member of the value list.
    NotIn,
    /// Substring containment.
    Contains,
    /// Field starts with the value.
    StartsWith,
    /// Field is JSON null or missing.
    IsNull,
    /// Field is neither JSON null nor missing.
    IsNotNull,
    /// Relevance-ish match; adapter maps to tsvector / multi_match / q.
    FullText,
}

/// A filter operand: one typed value (or bound set) to compare against.
#[derive(Clone, Debug)]
pub enum FilterValue {
    /// JSON null.
    Null,
    /// A boolean.
    Bool(bool),
    /// A 64-bit integer.
    Int(i64),
    /// A 64-bit float.
    Float(f64),
    /// A string.
    Str(String),
    /// An instant in UTC.
    DateTime(DateTime<Utc>),
    /// A list of candidate values.
    In(Vec<serde_json::Value>),
    /// An open/closed interval; missing bounds are unconstrained.
    Range {
        /// Exclusive lower bound.
        gt: Option<serde_json::Value>,
        /// Inclusive lower bound.
        gte: Option<serde_json::Value>,
        /// Exclusive upper bound.
        lt: Option<serde_json::Value>,
        /// Inclusive upper bound.
        lte: Option<serde_json::Value>,
    },
}

/// A filter tree: AND/OR/NOT over field predicates.
///
/// The tree is the minimum that makes ES `bool` queries, GraphQL `OR`, and
/// SQL `WHERE (a OR b) AND c` all expressible. List-view params build a tree
/// (filter sidebar → `And`; date ranges → `Range`); the adapter flattens or
/// nests as its source requires.
#[derive(Clone, Debug)]
pub enum FilterNode {
    /// A single field predicate.
    Field {
        /// Field name.
        field: String,
        /// Comparison operator.
        op: FilterOp,
        /// Operand value.
        value: FilterValue,
    },
    /// All children must match.
    And(Vec<FilterNode>),
    /// Any child may match.
    Or(Vec<FilterNode>),
    /// The child must not match.
    Not(Box<FilterNode>),
}

/// A single sort key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SortField {
    /// Field name.
    pub field: String,
    /// Direction.
    pub dir: SortDir,
    /// Null ordering.
    pub nulls: NullsOrder,
}

impl SortField {
    /// Ascending sort on `field`, default null ordering.
    pub fn asc(field: impl Into<String>) -> Self {
        Self {
            field: field.into(),
            dir: SortDir::Asc,
            nulls: NullsOrder::Default,
        }
    }

    /// Descending sort on `field`, default null ordering.
    pub fn desc(field: impl Into<String>) -> Self {
        Self {
            field: field.into(),
            dir: SortDir::Desc,
            nulls: NullsOrder::Default,
        }
    }
}

/// Sort direction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortDir {
    /// Ascending.
    Asc,
    /// Descending.
    Desc,
}

/// Where null values sort.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NullsOrder {
    /// Nulls first.
    First,
    /// Nulls last.
    Last,
    /// Adapter default (Postgres: nulls last ascending, first descending).
    Default,
}

/// What "search" means for this source; the engine renders the matching UX.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SearchMode {
    /// No search support.
    None,
    /// Exact match only.
    Exact,
    /// Substring match.
    Substring,
    /// Relevance-ish full-text match.
    FullText,
}

/// A search request: term across the listed fields.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchSpec {
    /// The search term.
    pub term: String,
    /// Fields to search (from the resource's `search_fields`).
    pub fields: Vec<String>,
}

/// Pagination mode: numbered offset pages or opaque cursors.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Pagination {
    /// `page` is 1-based; the classic numbered-pager contract.
    Offset {
        /// 1-based page number.
        page: usize,
        /// Rows per page.
        per_page: usize,
    },
    /// Opaque cursor; adapter-encoded, framework-blind.
    Cursor {
        /// Resume after this cursor (exclusive).
        after: Option<String>,
        /// Resume before this cursor (exclusive).
        before: Option<String>,
        /// Rows per page.
        per_page: usize,
    },
}

/// Opaque to the framework; the adapter encodes its own continuation state.
///
/// In-memory adapters encode an index; SQL adapters encode a keyset; HTTP
/// adapters pass through the source's cursor verbatim.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cursor(pub String);

/// A bounded, translatable read request.
///
/// `filter` already merges the user's filters with the policy scope — the
/// adapter never knows about teams, roles, or row scoping.
#[derive(Clone, Debug)]
pub struct Query {
    /// How to page.
    pub pagination: Pagination,
    /// Multi-column sort, in priority order.
    pub sort: Vec<SortField>,
    /// User filters ∧ policy scope, already merged.
    pub filter: Option<FilterNode>,
    /// Search request, if any.
    pub search: Option<SearchSpec>,
    /// Projection — `None` = all fields.
    pub projection: Option<Vec<String>>,
}

/// One page of results, echoing the request's pagination mode.
///
/// `total: Some(n)` → numbered pages; `total: None` → prev/next driven purely
/// by cursors. The framework renders exactly one of those two pagers.
#[derive(Clone, Debug)]
pub struct Page<E> {
    /// The page's rows.
    pub items: Vec<E>,
    /// `None` when the source can't count cheaply (APIs, ES).
    pub total: Option<u64>,
    /// Cursor for the next page, if any.
    pub next: Option<Cursor>,
    /// Cursor for the previous page, if any.
    pub prev: Option<Cursor>,
    /// Echo of the request pagination; tells the UI which render mode to use.
    pub pagination: Pagination,
}
