//! The reference `DataAdapter` implementation: an in-memory store.
//!
//! Ships with the framework per `00` §5.9 — it powers the demo app (no
//! Postgres required), the test suite, and is the proof that the trait
//! contract is implementable as specified. Every default and every
//! capability is exercised against it in CI.

use std::any::TypeId;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::marker::PhantomData;
use std::sync::Arc;

use parking_lot::RwLock;

use async_trait::async_trait;
use chrono::{DateTime, Datelike, Duration, Timelike, Utc};
use serde::{Serialize, de::DeserializeOwned};

use crate::adapter::{DataAdapter, TxAdapter};
use crate::aggregation::{Aggregation, AggregationResult, Bucket, GroupBy, Interval, Measure};
use crate::capabilities::{
    AggregationCapability, Capabilities, ConcurrencySupport, PaginationModes, WriteCapability,
};
use crate::error::DataError;
use crate::query::{
    Cursor, FilterNode, FilterOp, FilterValue, NullsOrder, Page, Pagination, Query, SearchMode,
    SortDir,
};
use crate::write::{Mutation, WriteContext};

/// A `HashMap`-backed adapter with full capabilities: writes, transactions,
/// aggregation, streaming.
///
/// Internal storage is JSON keyed by id string; typed entities convert at
/// the API boundary, so `E = serde_json::Value` works identically.
pub struct InMemoryAdapter<E> {
    store: Arc<RwLock<HashMap<String, serde_json::Value>>>,
    _marker: PhantomData<E>,
}

/// The transaction handle `InMemoryAdapter::begin` returns.
///
/// Snapshots the store at `begin`; `commit` swaps the snapshot into the
/// shared map, `rollback` discards it. No lock is held across awaits.
pub struct InMemoryTx<E> {
    snapshot: HashMap<String, serde_json::Value>,
    shared: Arc<RwLock<HashMap<String, serde_json::Value>>>,
    _marker: PhantomData<E>,
}

impl<E: Serialize + DeserializeOwned + Send + Sync + 'static> InMemoryAdapter<E> {
    /// An empty adapter.
    pub fn new() -> Self {
        return Self {
            store: Arc::new(RwLock::new(HashMap::new())),
            _marker: PhantomData,
        };
    }

    /// Seed one entity by id. `Conflict` if the id exists.
    pub fn insert(&self, id: String, entity: E) -> Result<(), DataError> {
        let value =
            serde_json::to_value(entity).map_err(|e| return DataError::Internal(Box::new(e)))?;
        let mut store = self.store.write();
        if store.contains_key(&id) {
            return Err(DataError::Conflict);
        }
        store.insert(id, value);
        return Ok(());
    }

    /// All rows as JSON values, in id order (deterministic tie-break for
    /// stable list output).
    fn rows(&self) -> Vec<serde_json::Value> {
        let store = self.store.read();
        let mut rows: Vec<_> = store.values().cloned().collect();
        rows.sort_by(|a, b| return value_total_cmp(&a["id"], &b["id"]));
        return rows;
    }
}

impl<E> Default for InMemoryAdapter<E>
where
    E: Serialize + DeserializeOwned + Send + Sync + 'static,
{
    fn default() -> Self {
        return Self::new();
    }
}

// ---------------------------------------------------------------------------
// Value comparison semantics (private helpers)
// ---------------------------------------------------------------------------

/// The field's JSON value as a float, when numeric.
fn as_f64(v: &serde_json::Value) -> Option<f64> {
    return v.as_f64();
}

/// Numeric-or-lexicographic comparison; `None` for incomparable pairs.
fn cmp_values(a: &serde_json::Value, b: &serde_json::Value) -> Option<Ordering> {
    if let (Some(x), Some(y)) = (as_f64(a), as_f64(b)) {
        return x.partial_cmp(&y);
    }
    match (a, b) {
        (serde_json::Value::String(x), serde_json::Value::String(y)) => return Some(x.cmp(y)),
        _ => return None,
    }
}

/// Total order over JSON values: numbers first (by value), then null, bools,
/// strings, and everything else by serialized form.
fn value_total_cmp(a: &serde_json::Value, b: &serde_json::Value) -> Ordering {
    if let (Some(x), Some(y)) = (as_f64(a), as_f64(b)) {
        return x.total_cmp(&y);
    }
    match (a, b) {
        (serde_json::Value::Null, serde_json::Value::Null) => return Ordering::Equal,
        (serde_json::Value::Null, _) => return Ordering::Less,
        (_, serde_json::Value::Null) => return Ordering::Greater,
        (serde_json::Value::Bool(x), serde_json::Value::Bool(y)) => return x.cmp(y),
        (serde_json::Value::String(x), serde_json::Value::String(y)) => return x.cmp(y),
        _ => return a.to_string().cmp(&b.to_string()),
    }
}

/// Numeric equality with int/float coercion (exact; no epsilon).
fn value_eq(a: &serde_json::Value, b: &serde_json::Value) -> bool {
    match (as_f64(a), as_f64(b)) {
        (Some(x), Some(y)) => return x == y,
        _ => return a == b,
    }
}

/// Parse a JSON string as an RFC 3339 instant.
fn parse_dt(v: &serde_json::Value) -> Option<DateTime<Utc>> {
    let s = v.as_str()?;
    return DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|d| return d.with_timezone(&Utc));
}

/// A field's value: `None` when the row is not an object or the key is
/// missing.
fn field_of<'a>(row: &'a serde_json::Value, field: &str) -> Option<&'a serde_json::Value> {
    return row.get(field);
}

/// `Eq`-style match for a non-range operand.
fn eq_match(field: Option<&serde_json::Value>, value: &FilterValue) -> bool {
    match value {
        FilterValue::Null => return field.is_none() || field == Some(&serde_json::Value::Null),
        FilterValue::Bool(b) => return field == Some(&serde_json::Value::Bool(*b)),
        FilterValue::Int(i) => return field.and_then(as_f64) == Some(*i as f64),
        FilterValue::Float(f) => return field.and_then(as_f64) == Some(*f),
        FilterValue::Str(s) => return field == Some(&serde_json::Value::String(s.clone())),
        FilterValue::DateTime(dt) => return field.and_then(parse_dt) == Some(*dt),
        // `In` and `Range` are handled by their operators.
        FilterValue::In(_) | FilterValue::Range { .. } => return false,
    }
}

/// Range check: numeric bounds, missing bound = unconstrained.
fn range_match(field: Option<&serde_json::Value>, range: &FilterValue) -> bool {
    let FilterValue::Range { gt, gte, lt, lte } = range else {
        return false;
    };
    let Some(v) = field.and_then(as_f64) else {
        return false;
    };
    // A non-numeric bound is treated as unconstrained.
    let gt_ok = gt.as_ref().and_then(as_f64).is_none_or(|b| return v > b);
    let gte_ok = gte.as_ref().and_then(as_f64).is_none_or(|b| return v >= b);
    let lt_ok = lt.as_ref().and_then(as_f64).is_none_or(|b| return v < b);
    let lte_ok = lte.as_ref().and_then(as_f64).is_none_or(|b| return v <= b);
    return gt_ok && gte_ok && lt_ok && lte_ok;
}

/// One predicate over one row.
fn eval_field(row: &serde_json::Value, field: &str, op: FilterOp, value: &FilterValue) -> bool {
    let fv = field_of(row, field);
    match op {
        FilterOp::Eq => match value {
            FilterValue::Range { .. } => return range_match(fv, value),
            _ => return eq_match(fv, value),
        },
        // Missing field is not equal to anything, so `Ne` is true.
        FilterOp::Ne => return !eq_match(fv, value),
        FilterOp::Gt | FilterOp::Gte | FilterOp::Lt | FilterOp::Lte => {
            let Some(fv) = fv else { return false };
            let bound = match value {
                FilterValue::Int(i) => Some(serde_json::Value::Number((*i).into())),
                FilterValue::Float(f) => {
                    serde_json::Number::from_f64(*f).map(serde_json::Value::Number)
                }
                FilterValue::Str(s) => Some(serde_json::Value::String(s.clone())),
                FilterValue::DateTime(dt) => Some(serde_json::Value::String(dt.to_rfc3339())),
                _ => None,
            };
            let Some(bound) = bound else { return false };
            let Some(ord) = cmp_values(fv, &bound) else {
                return false;
            };
            match op {
                FilterOp::Gt => return ord == Ordering::Greater,
                FilterOp::Gte => return ord != Ordering::Less,
                FilterOp::Lt => return ord == Ordering::Less,
                FilterOp::Lte => return ord != Ordering::Greater,
                _ => unreachable!(),
            }
        }
        FilterOp::In => {
            return fv.is_some_and(|v| {
                let FilterValue::In(candidates) = value else {
                    return false;
                };
                return candidates.iter().any(|c| return value_eq(v, c));
            });
        }
        FilterOp::NotIn => {
            let FilterValue::In(candidates) = value else {
                return false;
            };
            return !fv.is_some_and(|v| return candidates.iter().any(|c| return value_eq(v, c)));
        }
        FilterOp::Contains => {
            return fv.and_then(serde_json::Value::as_str).is_some_and(|s| {
                let FilterValue::Str(needle) = value else {
                    return false;
                };
                return s.contains(needle);
            });
        }
        FilterOp::StartsWith => {
            return fv.and_then(serde_json::Value::as_str).is_some_and(|s| {
                let FilterValue::Str(prefix) = value else {
                    return false;
                };
                return s.starts_with(prefix);
            });
        }
        FilterOp::IsNull => return fv.is_none() || fv == Some(&serde_json::Value::Null),
        FilterOp::IsNotNull => return fv.is_some() && fv != Some(&serde_json::Value::Null),
        FilterOp::FullText => {
            return fv.and_then(serde_json::Value::as_str).is_some_and(|s| {
                let FilterValue::Str(term) = value else {
                    return false;
                };
                return s.to_lowercase().contains(&term.to_lowercase());
            });
        }
    }
}

/// Evaluate a filter tree against one row, with short-circuiting.
fn eval_filter(node: &FilterNode, row: &serde_json::Value) -> bool {
    match node {
        FilterNode::Field { field, op, value } => return eval_field(row, field, *op, value),
        FilterNode::And(children) => return children.iter().all(|c| return eval_filter(c, row)),
        FilterNode::Or(children) => return children.iter().any(|c| return eval_filter(c, row)),
        FilterNode::Not(child) => return !eval_filter(child, row),
    }
}

/// Rows matching the optional filter.
fn filtered(rows: &[serde_json::Value], filter: Option<&FilterNode>) -> Vec<serde_json::Value> {
    match filter {
        Some(node) => {
            return rows
                .iter()
                .filter(|r| return eval_filter(node, r))
                .cloned()
                .collect();
        }
        None => return rows.to_vec(),
    }
}

/// Case-insensitive substring over any of the search fields' string values.
fn matches_search(row: &serde_json::Value, term: &str, fields: &[String]) -> bool {
    let term = term.to_lowercase();
    return fields.iter().any(|f| {
        return row
            .get(f)
            .and_then(serde_json::Value::as_str)
            .is_some_and(|s| return s.to_lowercase().contains(&term));
    });
}

/// Compare two field values for one sort key, honoring null placement.
fn sort_cmp(
    a: Option<&serde_json::Value>,
    b: Option<&serde_json::Value>,
    dir: SortDir,
    nulls: NullsOrder,
) -> Ordering {
    let a_null = a.is_none_or(serde_json::Value::is_null);
    let b_null = b.is_none_or(serde_json::Value::is_null);

    if a_null || b_null {
        let null_first = match nulls {
            NullsOrder::First => true,
            NullsOrder::Last => false,
            // Postgres semantics: nulls last ascending, first descending.
            NullsOrder::Default => matches!(dir, SortDir::Desc),
        };
        return match (a_null, b_null) {
            (true, true) => Ordering::Equal,
            (true, false) => {
                if null_first {
                    Ordering::Less
                } else {
                    Ordering::Greater
                }
            }
            (false, true) => {
                if null_first {
                    Ordering::Greater
                } else {
                    Ordering::Less
                }
            }
            (false, false) => Ordering::Equal,
        };
    }

    let ord = cmp_values(a.unwrap(), b.unwrap())
        .unwrap_or_else(|| return a.unwrap().to_string().cmp(&b.unwrap().to_string()));
    match dir {
        SortDir::Asc => return ord,
        SortDir::Desc => return ord.reverse(),
    }
}

/// Stable multi-key sort (ties break by id via `rows` order).
fn sort_rows(rows: &mut Vec<serde_json::Value>, sort: &[crate::query::SortField]) {
    let mut order: Vec<usize> = (0..rows.len()).collect();
    order.sort_by(|&ia, &ib| {
        for sf in sort {
            let ord = sort_cmp(
                field_of(&rows[ia], &sf.field),
                field_of(&rows[ib], &sf.field),
                sf.dir,
                sf.nulls,
            );
            if ord != Ordering::Equal {
                return ord;
            }
        }
        return ia.cmp(&ib);
    });
    // Apply the permutation.
    let old = std::mem::take(rows);
    *rows = order.into_iter().map(|i| return old[i].clone()).collect();
}

// ---------------------------------------------------------------------------
// Cursor encoding (in-memory cursors are base64-encoded decimal indices —
// framework-blind, documented as an implementation detail)
// ---------------------------------------------------------------------------

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn encode_b64(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        out.push(B64[(b[0] >> 2) as usize] as char);
        out.push(B64[((b[0] & 0x03) << 4 | b[1] >> 4) as usize] as char);
        out.push(if chunk.len() > 1 {
            B64[((b[1] & 0x0f) << 2 | b[2] >> 6) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            B64[(b[2] & 0x3f) as usize] as char
        } else {
            '='
        });
    }
    return out;
}

fn decode_b64(s: &str) -> Option<Vec<u8>> {
    fn val(c: u8) -> Option<u8> {
        match c {
            b'A'..=b'Z' => return Some(c - b'A'),
            b'a'..=b'z' => return Some(c - b'a' + 26),
            b'0'..=b'9' => return Some(c - b'0' + 52),
            b'+' => return Some(62),
            b'/' => return Some(63),
            _ => return None,
        }
    }
    let mut out = Vec::with_capacity(s.len() / 4 * 3);
    let mut acc: u32 = 0;
    let mut bits = 0u32;
    for c in s.bytes() {
        if c == b'=' {
            break;
        }
        let v = val(c)?;
        acc = acc << 6 | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    return Some(out);
}

fn cursor_index(c: &Cursor) -> Option<usize> {
    let bytes = decode_b64(&c.0)?;
    return std::str::from_utf8(&bytes).ok()?.parse().ok();
}

fn index_cursor(index: usize) -> Cursor {
    return Cursor(encode_b64(index.to_string().as_bytes()));
}

// ---------------------------------------------------------------------------
// Mutations against a map (shared by the adapter and the transaction)
// ---------------------------------------------------------------------------

/// Apply one mutation to `map` with the adapter's exact rules. Returns the
/// stored id for `Create` (which may have generated one), `None` otherwise.
fn apply_one(
    map: &mut HashMap<String, serde_json::Value>,
    m: &Mutation<String>,
) -> Result<Option<String>, DataError> {
    match m {
        Mutation::Create { data } => {
            let serde_json::Value::Object(obj) = data else {
                return Err(DataError::Validation(
                    "create data must be a JSON object".into(),
                ));
            };
            let id = match obj.get("id") {
                Some(serde_json::Value::String(s)) => s.clone(),
                Some(serde_json::Value::Number(n)) => n.to_string(),
                _ => uuid::Uuid::new_v4().to_string(),
            };
            if map.contains_key(&id) {
                return Err(DataError::Conflict);
            }
            let mut record = obj.clone();
            if !record.get("id").is_some_and(serde_json::Value::is_string) {
                record.insert("id".into(), serde_json::Value::String(id.clone()));
            }
            map.insert(id.clone(), serde_json::Value::Object(record));
            return Ok(Some(id));
        }
        Mutation::Update { id, patch } => {
            let Some(record) = map.get_mut(id) else {
                return Err(DataError::NotFound);
            };
            let serde_json::Value::Object(record) = record else {
                return Err(DataError::Validation("record is not a JSON object".into()));
            };
            let serde_json::Value::Object(patch) = patch else {
                return Err(DataError::Validation("patch must be a JSON object".into()));
            };
            for (k, v) in patch {
                if k == "id" {
                    continue; // "id" is immutable
                }
                record.insert(k.clone(), v.clone());
            }
            return Ok(None);
        }
        Mutation::Delete { id } => {
            if map.remove(id).is_none() {
                return Err(DataError::NotFound);
            }
            return Ok(None);
        }
        Mutation::Upsert { id, data } => {
            match apply_one(map, &Mutation::Create { data: data.clone() }) {
                Ok(id) => return Ok(id),
                Err(DataError::Conflict) => {
                    return apply_one(
                        map,
                        &Mutation::Update {
                            id: id.clone(),
                            patch: data.clone(),
                        },
                    );
                }
                Err(e) => return Err(e),
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Aggregation helpers
// ---------------------------------------------------------------------------

/// The measure over a row set; `None` for empty/absent numeric input
/// (except `Count` and `Distinct`, which are always defined).
fn measure_value(measure: &Measure, rows: &[serde_json::Value]) -> Option<f64> {
    match measure {
        Measure::Count => return Some(rows.len() as f64),
        Measure::Sum(field) => {
            let sum: f64 = rows
                .iter()
                .filter_map(|r| return r.get(field).and_then(as_f64))
                .sum();
            if rows
                .iter()
                .any(|r| return r.get(field).and_then(as_f64).is_some())
            {
                return Some(sum);
            } else {
                return None;
            }
        }
        Measure::Avg(field) => {
            let vals: Vec<f64> = rows
                .iter()
                .filter_map(|r| return r.get(field).and_then(as_f64))
                .collect();
            if vals.is_empty() {
                return None;
            } else {
                return Some(vals.iter().sum::<f64>() / vals.len() as f64);
            }
        }
        Measure::Min(field) => {
            return rows
                .iter()
                .filter_map(|r| return r.get(field).and_then(as_f64))
                .min_by(|a, b| return a.total_cmp(b));
        }
        Measure::Max(field) => {
            return rows
                .iter()
                .filter_map(|r| return r.get(field).and_then(as_f64))
                .max_by(|a, b| return a.total_cmp(b));
        }
        Measure::Distinct(field) => {
            let mut seen = Vec::new();
            for r in rows {
                if let Some(v) = r
                    .get(field)
                    .filter(|v| return !seen.iter().any(|s| return value_eq(s, v)))
                {
                    seen.push(v.clone());
                }
            }
            return Some(seen.len() as f64);
        }
    }
}

/// Truncate an instant to the interval's bucket boundary.
fn truncate(dt: DateTime<Utc>, interval: &Interval) -> DateTime<Utc> {
    let naive = dt.naive_utc();
    let date = naive.date();
    let midnight = |d: chrono::NaiveDate| {
        return DateTime::from_naive_utc_and_offset(d.and_hms_opt(0, 0, 0).unwrap(), Utc);
    };
    match interval {
        Interval::Minute => {
            let t = naive
                .time()
                .with_second(0)
                .unwrap()
                .with_nanosecond(0)
                .unwrap();
            return DateTime::from_naive_utc_and_offset(naive.date().and_time(t), Utc);
        }
        Interval::Hour => {
            let t = naive
                .time()
                .with_minute(0)
                .unwrap()
                .with_second(0)
                .unwrap()
                .with_nanosecond(0)
                .unwrap();
            return DateTime::from_naive_utc_and_offset(naive.date().and_time(t), Utc);
        }
        Interval::Day => return midnight(date),
        Interval::Week => {
            let monday = date - Duration::days(date.weekday().num_days_from_monday() as i64);
            return midnight(monday);
        }
        Interval::Month => {
            return midnight(
                chrono::NaiveDate::from_ymd_opt(date.year(), date.month(), 1).unwrap(),
            );
        }
        Interval::Quarter => {
            let month = ((date.month() - 1) / 3) * 3 + 1;
            return midnight(chrono::NaiveDate::from_ymd_opt(date.year(), month, 1).unwrap());
        }
        Interval::Year => {
            return midnight(chrono::NaiveDate::from_ymd_opt(date.year(), 1, 1).unwrap());
        }
    }
}

/// Group rows by a field's JSON value (`Null` key for missing/null).
fn group_by_field(
    rows: &[serde_json::Value],
    field: &str,
) -> Vec<(serde_json::Value, Vec<serde_json::Value>)> {
    let mut groups: Vec<(serde_json::Value, Vec<serde_json::Value>)> = Vec::new();
    for row in rows {
        let key = row.get(field).cloned().unwrap_or(serde_json::Value::Null);
        match groups.iter_mut().find(|(k, _)| return value_eq(k, &key)) {
            Some((_, group)) => group.push(row.clone()),
            None => groups.push((key, vec![row.clone()])),
        }
    }
    groups.sort_by(|(a, _), (b, _)| return value_total_cmp(a, b));
    return groups;
}

/// Group rows into date-histogram buckets; unparseable/missing values are
/// skipped. Bucket keys are truncated RFC 3339 strings.
fn group_by_histogram(
    rows: &[serde_json::Value],
    field: &str,
    interval: &Interval,
) -> Vec<(serde_json::Value, Vec<serde_json::Value>)> {
    let mut groups: Vec<(serde_json::Value, Vec<serde_json::Value>)> = Vec::new();
    for row in rows {
        let Some(dt) = row.get(field).and_then(parse_dt) else {
            continue;
        };
        let key = serde_json::Value::String(truncate(dt, interval).to_rfc3339());
        match groups.iter_mut().find(|(k, _)| return *k == key) {
            Some((_, group)) => group.push(row.clone()),
            None => groups.push((key, vec![row.clone()])),
        }
    }
    groups.sort_by(|(a, _), (b, _)| return value_total_cmp(a, b));
    return groups;
}

// ---------------------------------------------------------------------------
// DataAdapter impl
// ---------------------------------------------------------------------------

#[async_trait]
impl<E> DataAdapter<E> for InMemoryAdapter<E>
where
    E: Serialize + DeserializeOwned + Send + Sync + 'static,
{
    fn capabilities(&self) -> Capabilities {
        return Capabilities {
            pagination: PaginationModes::Both,
            totals: true,
            write: WriteCapability::Crud,
            transactions: true,
            search: SearchMode::FullText,
            filter_ops: vec![
                FilterOp::Eq,
                FilterOp::Ne,
                FilterOp::Gt,
                FilterOp::Gte,
                FilterOp::Lt,
                FilterOp::Lte,
                FilterOp::In,
                FilterOp::NotIn,
                FilterOp::Contains,
                FilterOp::StartsWith,
                FilterOp::IsNull,
                FilterOp::IsNotNull,
                FilterOp::FullText,
            ],
            sort: true,
            aggregation: AggregationCapability::Histogram,
            concurrency: ConcurrencySupport::None,
            streaming: true,
            schema_discovery: false,
        };
    }

    #[allow(clippy::implicit_return)]
    async fn list(&self, query: &Query) -> Result<Page<E>, DataError> {
        // Filter → search → sort → projection → pagination.
        let mut rows = filtered(&self.rows(), query.filter.as_ref());

        if let Some(search) = &query.search {
            rows.retain(|r| matches_search(r, &search.term, &search.fields));
        }

        if !query.sort.is_empty() {
            sort_rows(&mut rows, &query.sort);
        }

        let is_value_entity = TypeId::of::<E>() == TypeId::of::<serde_json::Value>();
        if is_value_entity && let Some(projection) = &query.projection {
            for row in &mut rows {
                if let serde_json::Value::Object(map) = row {
                    let trimmed: serde_json::Map<String, serde_json::Value> = projection
                        .iter()
                        .filter_map(|k| map.get(k).map(|v| (k.clone(), v.clone())))
                        .collect();
                    *map = trimmed;
                }
            }
        }

        let len = rows.len();
        let (slice, total, next, prev) = match &query.pagination {
            Pagination::Offset { page, per_page } => {
                let page = (*page).max(1);
                let start = (page - 1) * per_page;
                let end = (start + per_page).min(len);
                let slice = if start >= len {
                    Vec::new()
                } else {
                    rows[start..end].to_vec()
                };
                let next = if end < len {
                    Some(index_cursor(end))
                } else {
                    None
                };
                let prev = if page > 1 {
                    Some(index_cursor((page - 2) * per_page))
                } else {
                    None
                };
                (slice, Some(len as u64), next, prev)
            }
            Pagination::Cursor {
                after,
                before,
                per_page,
            } => {
                let start = match (after, before) {
                    (Some(a), _) => cursor_index(&Cursor(a.clone())).unwrap_or(0),
                    (None, Some(b)) => cursor_index(&Cursor(b.clone()))
                        .map(|i| i.saturating_sub(*per_page))
                        .unwrap_or(0),
                    (None, None) => 0,
                };
                let end = (start + per_page).min(len);
                let slice = if start >= len {
                    Vec::new()
                } else {
                    rows[start..end].to_vec()
                };
                let next = if end < len {
                    Some(index_cursor(end))
                } else {
                    None
                };
                let prev = if start > 0 {
                    Some(index_cursor(start.saturating_sub(*per_page)))
                } else {
                    None
                };
                // Cursor mode never counts.
                (slice, None, next, prev)
            }
        };

        let items = slice
            .into_iter()
            .map(|v| serde_json::from_value(v).map_err(|e| DataError::Internal(Box::new(e))))
            .collect::<Result<Vec<E>, DataError>>()?;

        Ok(Page {
            items,
            total,
            next,
            prev,
            pagination: query.pagination.clone(),
        })
    }

    #[allow(clippy::implicit_return)]
    async fn get(&self, id: &String) -> Result<Option<E>, DataError> {
        let row = self.store.read().get(id).cloned();
        row.map(|v| serde_json::from_value(v).map_err(|e| DataError::Internal(Box::new(e))))
            .transpose()
    }

    #[allow(clippy::implicit_return)]
    async fn create(
        &self,
        data: serde_json::Value,
        _ctx: &WriteContext<'_>,
    ) -> Result<E, DataError> {
        let mut map = self.store.write();
        let stored_id = apply_one(&mut map, &Mutation::Create { data })?;
        // The stored record always carries a string "id".
        let row = map.get(&stored_id.unwrap()).cloned().unwrap();
        serde_json::from_value(row).map_err(|e| DataError::Internal(Box::new(e)))
    }

    #[allow(clippy::implicit_return)]
    async fn update(
        &self,
        id: &String,
        patch: serde_json::Value,
        _ctx: &WriteContext<'_>,
    ) -> Result<E, DataError> {
        let mut map = self.store.write();
        apply_one(
            &mut map,
            &Mutation::Update {
                id: id.clone(),
                patch,
            },
        )?;
        let row = map.get(id).cloned().unwrap();
        serde_json::from_value(row).map_err(|e| DataError::Internal(Box::new(e)))
    }

    #[allow(clippy::implicit_return)]
    async fn delete(&self, id: &String, _ctx: &WriteContext<'_>) -> Result<(), DataError> {
        let mut map = self.store.write();
        apply_one(&mut map, &Mutation::Delete { id: id.clone() }).map(|_| ())
    }

    #[allow(clippy::implicit_return)]
    async fn begin(&self) -> Result<Box<dyn TxAdapter<E>>, DataError> {
        let snapshot = self.store.read().clone();
        Ok(Box::new(InMemoryTx {
            snapshot,
            shared: self.store.clone(),
            _marker: PhantomData,
        }))
    }

    #[allow(clippy::implicit_return)]
    async fn aggregate(&self, agg: &Aggregation) -> Result<AggregationResult, DataError> {
        let rows = filtered(&self.rows(), agg.filter.as_ref());

        let Some(group_by) = &agg.group_by else {
            return Ok(AggregationResult {
                value: measure_value(&agg.measure, &rows),
                buckets: Vec::new(),
            });
        };

        let groups = match group_by {
            GroupBy::Field(field) => group_by_field(&rows, field),
            GroupBy::DateHistogram { field, interval } => {
                group_by_histogram(&rows, field, interval)
            }
        };

        let mut buckets: Vec<Bucket> = groups
            .into_iter()
            .map(|(key, group)| Bucket {
                key,
                value: measure_value(&agg.measure, &group).unwrap_or(0.0),
            })
            .collect();
        if let Some(limit) = agg.limit {
            buckets.truncate(limit);
        }

        Ok(AggregationResult {
            value: None,
            buckets,
        })
    }

    // `get_many`, `apply_mutations`, `stream`, `describe`, `validate`:
    // trait defaults, per the reference-implementation mandate.

    // `describe` → default `Unsupported`: the in-memory store has no schema
    // to introspect. `validate` → default `Ok(())`: nothing to check.
}

// ---------------------------------------------------------------------------
// TxAdapter impl
// ---------------------------------------------------------------------------

#[async_trait]
#[allow(clippy::implicit_return)]
impl<E> TxAdapter<E> for InMemoryTx<E>
where
    E: Serialize + DeserializeOwned + Send + Sync + 'static,
{
    async fn get(&self, id: &String) -> Result<Option<E>, DataError> {
        self.snapshot
            .get(id)
            .cloned()
            .map(|v| serde_json::from_value(v).map_err(|e| DataError::Internal(Box::new(e))))
            .transpose()
    }

    async fn apply(&mut self, mutations: &[Mutation<String>]) -> Result<(), DataError> {
        for m in mutations {
            apply_one(&mut self.snapshot, m)?;
        }
        Ok(())
    }

    async fn commit(self: Box<Self>) -> Result<(), DataError> {
        let InMemoryTx {
            snapshot, shared, ..
        } = *self;
        *shared.write() = snapshot;
        Ok(())
    }

    async fn rollback(self: Box<Self>) -> Result<(), DataError> {
        // Dropping the snapshot discards the transaction's changes.
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;

    use crate::aggregation::Interval;
    use crate::query::{SortDir, SortField};

    type Adapter = InMemoryAdapter<serde_json::Value>;

    fn adapter() -> Adapter {
        return Adapter::new();
    }

    /// Five stores: 1, 2, 4 active; 3 inactive; 5 archived.
    fn seed(a: &Adapter) {
        for (id, name, status, revenue, created_at) in [
            ("1", "Alpha", "active", 100, "2026-08-10T09:30:00Z"),
            ("2", "Beta", "active", 250, "2026-08-11T23:59:00Z"),
            ("3", "Gamma", "inactive", 50, "2026-08-12T00:01:00Z"),
            ("4", "Delta", "active", 75, "2026-08-12T12:00:00Z"),
            ("5", "Epsilon", "archived", 200, "2026-08-13T08:00:00Z"),
        ] {
            a.insert(
                id.into(),
                serde_json::json!({
                    "id": id, "name": name, "status": status,
                    "revenue": revenue, "created_at": created_at,
                }),
            )
            .unwrap();
        }
    }

    fn no_page() -> Query {
        return Query {
            pagination: Pagination::Offset {
                page: 1,
                per_page: 100,
            },
            sort: vec![],
            filter: None,
            search: None,
            projection: None,
        };
    }

    fn field_node(field: &str, op: FilterOp, value: FilterValue) -> FilterNode {
        return FilterNode::Field {
            field: field.into(),
            op,
            value,
        };
    }

    #[test]
    fn insert_duplicate_conflicts() {
        let a = adapter();
        a.insert("1".into(), serde_json::json!({"id": "1"}))
            .unwrap();
        assert!(matches!(
            a.insert("1".into(), serde_json::json!({"id": "1"})),
            Err(DataError::Conflict)
        ));
    }

    #[test]
    fn list_returns_all_without_filters() {
        let a = adapter();
        seed(&a);
        let page = futures::executor::block_on(a.list(&no_page())).unwrap();
        assert_eq!(page.items.len(), 5);
        assert_eq!(page.total, Some(5));
        assert!(page.next.is_none());
        assert!(page.prev.is_none());
    }

    #[test]
    fn list_filters_with_eq_and_total() {
        let a = adapter();
        seed(&a);
        let mut q = no_page();
        q.filter = Some(field_node(
            "status",
            FilterOp::Eq,
            FilterValue::Str("active".into()),
        ));
        let page = futures::executor::block_on(a.list(&q)).unwrap();
        assert_eq!(page.items.len(), 3);
        assert_eq!(page.total, Some(3));
    }

    #[test]
    fn list_offset_pagination_pages() {
        let a = adapter();
        seed(&a);
        let mut q = no_page();
        q.pagination = Pagination::Offset {
            page: 2,
            per_page: 4,
        };
        let page = futures::executor::block_on(a.list(&q)).unwrap();
        assert_eq!(page.items.len(), 1);
        assert!(page.prev.is_some());
        assert!(page.next.is_none());
        assert_eq!(page.total, Some(5));
    }

    #[test]
    fn list_cursor_pagination_follows_next() {
        let a = adapter();
        seed(&a);
        let mut q = no_page();
        q.pagination = Pagination::Cursor {
            after: None,
            before: None,
            per_page: 2,
        };

        let p1 = futures::executor::block_on(a.list(&q)).unwrap();
        assert_eq!(p1.items.len(), 2);
        assert_eq!(p1.total, None);
        let c1 = p1.next.expect("next after page 1");

        q.pagination = Pagination::Cursor {
            after: Some(c1.0),
            before: None,
            per_page: 2,
        };
        let p2 = futures::executor::block_on(a.list(&q)).unwrap();
        assert_eq!(p2.items.len(), 2);
        assert_ne!(p2.items[0]["id"], p1.items[0]["id"]);
        let c2 = p2.next.expect("next after page 2");

        q.pagination = Pagination::Cursor {
            after: Some(c2.0),
            before: None,
            per_page: 2,
        };
        let p3 = futures::executor::block_on(a.list(&q)).unwrap();
        assert_eq!(p3.items.len(), 1);
        assert!(p3.next.is_none());
    }

    #[test]
    fn list_multi_column_sort_and_nulls_order() {
        let a = adapter();
        seed(&a);
        // Tweak rows 2 and 5 to have null priority.
        futures::executor::block_on(a.update(
            &"2".to_string(),
            serde_json::json!({"priority": serde_json::Value::Null}),
            &WriteContext {
                expected_version: None,
                idempotency_key: None,
                actor: None,
            },
        ))
        .unwrap();
        futures::executor::block_on(a.update(
            &"5".to_string(),
            serde_json::json!({"priority": serde_json::Value::Null}),
            &WriteContext {
                expected_version: None,
                idempotency_key: None,
                actor: None,
            },
        ))
        .unwrap();
        for (id, priority) in [("1", 2), ("3", 1), ("4", 3)] {
            futures::executor::block_on(a.update(
                &id.to_string(),
                serde_json::json!({"priority": priority}),
                &WriteContext {
                    expected_version: None,
                    idempotency_key: None,
                    actor: None,
                },
            ))
            .unwrap();
        }

        // Desc, default nulls (Postgres: nulls first).
        let mut q = no_page();
        q.sort = vec![SortField::desc("priority")];
        let page = futures::executor::block_on(a.list(&q)).unwrap();
        let names: Vec<_> = page
            .items
            .iter()
            .map(|r| return r["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, ["Beta", "Epsilon", "Delta", "Alpha", "Gamma"]);

        // Desc, explicit nulls last.
        q.sort = vec![SortField {
            field: "priority".into(),
            dir: SortDir::Desc,
            nulls: NullsOrder::Last,
        }];
        let page = futures::executor::block_on(a.list(&q)).unwrap();
        let names: Vec<_> = page
            .items
            .iter()
            .map(|r| return r["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, ["Delta", "Alpha", "Gamma", "Beta", "Epsilon"]);
    }

    #[test]
    fn list_and_or_not_filter_tree() {
        let a = adapter();
        seed(&a);
        let mut q = no_page();
        q.filter = Some(FilterNode::And(vec![
            FilterNode::Or(vec![
                field_node("status", FilterOp::Eq, FilterValue::Str("active".into())),
                field_node("status", FilterOp::Eq, FilterValue::Str("archived".into())),
            ]),
            FilterNode::Not(Box::new(field_node(
                "revenue",
                FilterOp::Lt,
                FilterValue::Int(100),
            ))),
        ]));
        let page = futures::executor::block_on(a.list(&q)).unwrap();
        let names: Vec<_> = page
            .items
            .iter()
            .map(|r| return r["name"].as_str().unwrap())
            .collect();
        assert_eq!(names.len(), 3);
        assert!(names.contains(&"Alpha"));
        assert!(names.contains(&"Beta"));
        assert!(names.contains(&"Epsilon"));
    }

    #[test]
    fn list_search_case_insensitive_substring() {
        let a = adapter();
        seed(&a);
        let mut q = no_page();
        q.search = Some(crate::query::SearchSpec {
            term: "ALP".into(),
            fields: vec!["name".into()],
        });
        let page = futures::executor::block_on(a.list(&q)).unwrap();
        assert_eq!(page.items.len(), 1);
        assert_eq!(page.items[0]["id"], "1");
    }

    #[test]
    fn list_projection_trims_value_entities() {
        let a = adapter();
        seed(&a);
        let mut q = no_page();
        q.projection = Some(vec!["id".into(), "name".into()]);
        let page = futures::executor::block_on(a.list(&q)).unwrap();
        for item in &page.items {
            let obj = item.as_object().unwrap();
            assert_eq!(obj.len(), 2);
            assert!(obj.contains_key("id"));
            assert!(obj.contains_key("name"));
        }
    }

    #[test]
    fn list_range_and_in_ops() {
        let a = adapter();
        seed(&a);

        let mut q = no_page();
        q.filter = Some(field_node(
            "revenue",
            FilterOp::Eq,
            FilterValue::Range {
                gt: None,
                gte: Some(serde_json::json!(100)),
                lt: Some(serde_json::json!(200)),
                lte: None,
            },
        ));
        let page = futures::executor::block_on(a.list(&q)).unwrap();
        assert_eq!(page.items.len(), 1);
        assert_eq!(page.items[0]["id"], "1");

        q.filter = Some(field_node(
            "revenue",
            FilterOp::In,
            FilterValue::In(vec![serde_json::json!(50), serde_json::json!(200)]),
        ));
        let page = futures::executor::block_on(a.list(&q)).unwrap();
        let ids: Vec<_> = page
            .items
            .iter()
            .map(|r| return r["id"].as_str().unwrap())
            .collect();
        assert_eq!(ids, ["3", "5"]);
    }

    #[test]
    fn create_assigns_generated_id() {
        let a = adapter();
        let created = futures::executor::block_on(a.create(
            serde_json::json!({"name": "New"}),
            &WriteContext {
                expected_version: None,
                idempotency_key: None,
                actor: None,
            },
        ))
        .unwrap();
        let id = created["id"].as_str().unwrap().to_string();
        assert!(!id.is_empty());
        let fetched = futures::executor::block_on(a.get(&id)).unwrap();
        assert_eq!(fetched.unwrap()["name"], "New");
    }

    #[test]
    fn create_duplicate_conflicts() {
        let a = adapter();
        seed(&a);
        let err = futures::executor::block_on(a.create(
            serde_json::json!({"id": "1", "name": "Clone"}),
            &WriteContext {
                expected_version: None,
                idempotency_key: None,
                actor: None,
            },
        ))
        .unwrap_err();
        assert!(matches!(err, DataError::Conflict));
    }

    #[test]
    fn update_merges_patch_and_keeps_id() {
        let a = adapter();
        seed(&a);
        let updated = futures::executor::block_on(a.update(
            &"1".to_string(),
            serde_json::json!({"name": "Alpha2", "id": "999"}),
            &WriteContext {
                expected_version: None,
                idempotency_key: None,
                actor: None,
            },
        ))
        .unwrap();
        assert_eq!(updated["name"], "Alpha2");
        assert_eq!(updated["id"], "1");
        assert_eq!(updated["status"], "active");
    }

    #[test]
    fn update_missing_not_found() {
        let a = adapter();
        let err = futures::executor::block_on(a.update(
            &"nope".to_string(),
            serde_json::json!({"name": "X"}),
            &WriteContext {
                expected_version: None,
                idempotency_key: None,
                actor: None,
            },
        ))
        .unwrap_err();
        assert!(matches!(err, DataError::NotFound));
    }

    #[test]
    fn delete_removes() {
        let a = adapter();
        seed(&a);
        futures::executor::block_on(a.delete(
            &"1".to_string(),
            &WriteContext {
                expected_version: None,
                idempotency_key: None,
                actor: None,
            },
        ))
        .unwrap();
        assert!(
            futures::executor::block_on(a.get(&"1".to_string()))
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn delete_missing_not_found() {
        let a = adapter();
        let err = futures::executor::block_on(a.delete(
            &"nope".to_string(),
            &WriteContext {
                expected_version: None,
                idempotency_key: None,
                actor: None,
            },
        ))
        .unwrap_err();
        assert!(matches!(err, DataError::NotFound));
    }

    #[test]
    fn get_many_default_skips_missing_in_order() {
        let a = adapter();
        seed(&a);
        let rows = futures::executor::block_on(a.get_many(&[
            "3".to_string(),
            "nope".to_string(),
            "1".to_string(),
        ]))
        .unwrap();
        let ids: Vec<_> = rows
            .iter()
            .map(|r| return r["id"].as_str().unwrap())
            .collect();
        assert_eq!(ids, ["3", "1"]);
    }

    #[test]
    fn apply_mutations_default_stops_on_first_error() {
        let a = adapter();
        seed(&a);
        let ctx = WriteContext {
            expected_version: None,
            idempotency_key: None,
            actor: None,
        };
        let err = futures::executor::block_on(a.apply_mutations(
            &[
                Mutation::Create {
                    data: serde_json::json!({"id": "9", "name": "Nine"}),
                },
                Mutation::Update {
                    id: "nope".into(),
                    patch: serde_json::json!({"name": "X"}),
                },
                Mutation::Create {
                    data: serde_json::json!({"id": "8", "name": "Eight"}),
                },
            ],
            &ctx,
        ))
        .unwrap_err();
        assert!(matches!(err, DataError::NotFound));
        // First mutation applied, third never reached.
        assert!(
            futures::executor::block_on(a.get(&"9".to_string()))
                .unwrap()
                .is_some()
        );
        assert!(
            futures::executor::block_on(a.get(&"8".to_string()))
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn apply_mutations_upsert_creates_then_updates() {
        let a = adapter();
        let ctx = WriteContext {
            expected_version: None,
            idempotency_key: None,
            actor: None,
        };
        futures::executor::block_on(a.apply_mutations(
            &[Mutation::Upsert {
                id: "7".into(),
                data: serde_json::json!({"id": "7", "name": "Seven"}),
            }],
            &ctx,
        ))
        .unwrap();
        futures::executor::block_on(a.apply_mutations(
            &[Mutation::Upsert {
                id: "7".into(),
                data: serde_json::json!({"id": "7", "name": "Seven2"}),
            }],
            &ctx,
        ))
        .unwrap();
        let row = futures::executor::block_on(a.get(&"7".to_string()))
            .unwrap()
            .unwrap();
        assert_eq!(row["name"], "Seven2");
        assert_eq!(a.store.read().len(), 1);
    }

    #[test]
    fn tx_commit_makes_changes_visible() {
        let a = adapter();
        seed(&a);
        let mut tx = futures::executor::block_on(a.begin()).unwrap();
        futures::executor::block_on(tx.apply(&[Mutation::Create {
            data: serde_json::json!({"id": "6", "name": "Six"}),
        }]))
        .unwrap();
        // Not visible before commit.
        assert!(
            futures::executor::block_on(a.get(&"6".to_string()))
                .unwrap()
                .is_none()
        );
        futures::executor::block_on(tx.commit()).unwrap();
        assert!(
            futures::executor::block_on(a.get(&"6".to_string()))
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn tx_rollback_discards() {
        let a = adapter();
        seed(&a);
        let mut tx = futures::executor::block_on(a.begin()).unwrap();
        futures::executor::block_on(tx.apply(&[Mutation::Create {
            data: serde_json::json!({"id": "6", "name": "Six"}),
        }]))
        .unwrap();
        futures::executor::block_on(tx.rollback()).unwrap();
        assert!(
            futures::executor::block_on(a.get(&"6".to_string()))
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn tx_apply_conflicts_on_duplicate_create() {
        let a = adapter();
        seed(&a);
        let mut tx = futures::executor::block_on(a.begin()).unwrap();
        let err = futures::executor::block_on(tx.apply(&[Mutation::Create {
            data: serde_json::json!({"id": "1", "name": "Clone"}),
        }]))
        .unwrap_err();
        assert!(matches!(err, DataError::Conflict));
    }

    #[test]
    fn aggregate_count_and_sum() {
        let a = adapter();
        seed(&a);
        let count = futures::executor::block_on(a.aggregate(&Aggregation {
            measure: Measure::Count,
            group_by: None,
            filter: None,
            sort: vec![],
            limit: None,
        }))
        .unwrap();
        assert_eq!(count.value, Some(5.0));
        assert!(count.buckets.is_empty());

        let sum = futures::executor::block_on(a.aggregate(&Aggregation {
            measure: Measure::Sum("revenue".into()),
            group_by: None,
            filter: None,
            sort: vec![],
            limit: None,
        }))
        .unwrap();
        assert_eq!(sum.value, Some(675.0));

        let sum = futures::executor::block_on(a.aggregate(&Aggregation {
            measure: Measure::Sum("revenue".into()),
            group_by: None,
            filter: Some(field_node(
                "status",
                FilterOp::Eq,
                FilterValue::Str("active".into()),
            )),
            sort: vec![],
            limit: None,
        }))
        .unwrap();
        assert_eq!(sum.value, Some(425.0));
    }

    #[test]
    fn aggregate_group_by_field_buckets() {
        let a = adapter();
        seed(&a);
        let result = futures::executor::block_on(a.aggregate(&Aggregation {
            measure: Measure::Count,
            group_by: Some(GroupBy::Field("status".into())),
            filter: None,
            sort: vec![],
            limit: None,
        }))
        .unwrap();
        assert_eq!(result.value, None);
        let buckets: Vec<_> = result
            .buckets
            .iter()
            .map(|b| return (b.key.as_str().unwrap(), b.value))
            .collect();
        assert_eq!(
            buckets,
            [("active", 3.0), ("archived", 1.0), ("inactive", 1.0)]
        );
    }

    #[test]
    fn aggregate_date_histogram_days() {
        let a = adapter();
        seed(&a);
        let result = futures::executor::block_on(a.aggregate(&Aggregation {
            measure: Measure::Count,
            group_by: Some(GroupBy::DateHistogram {
                field: "created_at".into(),
                interval: Interval::Day,
            }),
            filter: None,
            sort: vec![],
            limit: None,
        }))
        .unwrap();
        let buckets: Vec<_> = result
            .buckets
            .iter()
            .map(|b| return (b.key.as_str().unwrap().to_string(), b.value))
            .collect();
        assert_eq!(
            buckets,
            [
                ("2026-08-10T00:00:00+00:00".to_string(), 1.0),
                ("2026-08-11T00:00:00+00:00".to_string(), 1.0),
                ("2026-08-12T00:00:00+00:00".to_string(), 2.0),
                ("2026-08-13T00:00:00+00:00".to_string(), 1.0),
            ]
        );
    }

    #[test]
    fn aggregate_distinct() {
        let a = adapter();
        seed(&a);
        let result = futures::executor::block_on(a.aggregate(&Aggregation {
            measure: Measure::Distinct("status".into()),
            group_by: None,
            filter: None,
            sort: vec![],
            limit: None,
        }))
        .unwrap();
        assert_eq!(result.value, Some(3.0));
    }

    #[test]
    fn stream_default_yields_all_rows() {
        let a = adapter();
        seed(&a);
        let stream = futures::executor::block_on(a.stream(no_page()));
        let rows: Vec<_> = futures::executor::block_on(stream.collect::<Vec<_>>());
        assert_eq!(rows.len(), 5);
        assert!(rows.into_iter().all(|r| return r.is_ok()));
    }

    #[test]
    fn validate_ok_and_describe_unsupported() {
        let a = adapter();
        futures::executor::block_on(a.validate(&["name", "status"])).unwrap();
        assert!(matches!(
            futures::executor::block_on(a.describe()),
            Err(DataError::Unsupported)
        ));
    }
}
