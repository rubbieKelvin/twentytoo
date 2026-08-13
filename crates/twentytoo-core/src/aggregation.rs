//! Typed, metric-shaped aggregation.

use crate::query::{FilterNode, SortField};

/// One aggregation request, mapping mechanically to the source's native
/// dialect: SQL `COUNT/SUM/… GROUP BY date_trunc(…)`, ES `aggs`, ClickHouse
/// native.
#[derive(Clone, Debug)]
pub struct Aggregation {
    /// Which measure to compute.
    pub measure: Measure,
    /// Grouping, if any. `None` → a single scalar result.
    pub group_by: Option<GroupBy>,
    /// Row filter applied before aggregating.
    pub filter: Option<FilterNode>,
    /// Bucket ordering.
    pub sort: Vec<SortField>,
    /// Max buckets returned.
    pub limit: Option<usize>,
}

/// The five core metric types (`value`, `trend`, `partition`, `table`,
/// `progress`) all reduce to these measures.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Measure {
    /// Row count.
    Count,
    /// Sum of a numeric field.
    Sum(String),
    /// Average of a numeric field.
    Avg(String),
    /// Minimum of a numeric field.
    Min(String),
    /// Maximum of a numeric field.
    Max(String),
    /// Count of distinct values of a field.
    Distinct(String),
}

/// How to bucket rows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GroupBy {
    /// One bucket per distinct value of `field`.
    Field(String),
    /// One bucket per interval over `field`.
    DateHistogram {
        /// Field to histogram over.
        field: String,
        /// Bucket width.
        interval: Interval,
    },
}

/// Date-histogram bucket width.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Interval {
    /// One bucket per minute.
    Minute,
    /// One bucket per hour.
    Hour,
    /// One bucket per day.
    Day,
    /// One bucket per week.
    Week,
    /// One bucket per month.
    Month,
    /// One bucket per quarter.
    Quarter,
    /// One bucket per year.
    Year,
}

/// The result of an aggregation.
#[derive(Clone, Debug)]
pub struct AggregationResult {
    /// Scalar result — set when there is no `group_by`.
    pub value: Option<f64>,
    /// Bucketed result — set when there is a `group_by`.
    pub buckets: Vec<Bucket>,
}

/// One group: its key and the measure over that group.
#[derive(Clone, Debug)]
pub struct Bucket {
    /// Group key; `Null` for missing/null grouping fields.
    pub key: serde_json::Value,
    /// Measure value for the group.
    pub value: f64,
}
