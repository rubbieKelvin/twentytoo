//! Query-parameter parsing into filter trees.
//!
//! The pure half of list filtering: coercion by field kind and tree
//! assembly from declared specs. The handlers keep the orchestration
//! (adapter calls, pagination modes) around these functions.

use std::collections::HashMap;

use twentytoo_core::{Field, FieldKind, FilterNode, FilterOp, FilterValue};

/// Build the filter tree from declared specs + request params.
///
/// A spec is offered only when the source's `filter_ops` contains its
/// operator; unparseable param values are ignored (a bad filter is just no
/// filter). Range filters use `{field}_min` / `{field}_max` params on
/// numeric and date kinds.
pub fn build_filter<E>(
    specs: &[twentytoo_core::FilterSpec],
    fields: &[Field<E>],
    filter_ops: &[FilterOp],
    params: &HashMap<String, String>,
) -> Option<FilterNode> {
    let mut nodes: Vec<FilterNode> = Vec::new();

    for spec in specs {
        if !filter_ops.contains(&spec.op) {
            continue;
        }
        let Some(field) = fields.iter().find(|f| return f.name == spec.field) else {
            continue;
        };
        if let Some(raw) = params.get(spec.field)
            && let Some(value) = coerce(&field.kind, raw)
        {
            let op = match spec.op {
                FilterOp::In | FilterOp::NotIn => FilterValue::In(vec![value_to_json(&value)]),
                _ => value,
            };
            nodes.push(FilterNode::Field {
                field: spec.field.to_string(),
                op: spec.op,
                value: op,
            });
        }
    }

    // Ranges: {field}_min / {field}_max on numeric/date kinds.
    for field in fields {
        if !matches!(
            field.kind,
            FieldKind::Number | FieldKind::Currency | FieldKind::Date | FieldKind::DateTime
        ) {
            continue;
        }
        let min = params.get(&format!("{}_min", field.name));
        let max = params.get(&format!("{}_max", field.name));
        if min.is_none() && max.is_none() {
            continue;
        }
        let range = FilterValue::Range {
            gt: None,
            gte: min
                .and_then(|s| return coerce(&field.kind, s))
                .map(|v| return value_to_json(&v)),
            lt: None,
            lte: max
                .and_then(|s| return coerce(&field.kind, s))
                .map(|v| return value_to_json(&v)),
        };
        nodes.push(FilterNode::Field {
            field: field.name.to_string(),
            op: FilterOp::Gte,
            value: range,
        });
    }

    return match nodes.len() {
        0 => None,
        1 => nodes.pop(),
        _ => Some(FilterNode::And(nodes)),
    };
}

/// Coerce a query param to a typed filter value by field kind.
pub fn coerce(kind: &FieldKind, raw: &str) -> Option<FilterValue> {
    match kind {
        FieldKind::Number | FieldKind::Currency => {
            if let Ok(n) = raw.parse::<i64>() {
                return Some(FilterValue::Int(n));
            }
            return raw
                .parse::<f64>()
                .ok()
                .map(|n| return FilterValue::Float(n));
        }
        FieldKind::Boolean => match raw {
            "true" | "1" => return Some(FilterValue::Bool(true)),
            "false" | "0" => return Some(FilterValue::Bool(false)),
            _ => return None,
        },
        FieldKind::Date | FieldKind::DateTime => {
            return chrono::DateTime::parse_from_rfc3339(raw)
                .ok()
                .map(|dt| return FilterValue::DateTime(dt.with_timezone(&chrono::Utc)));
        }
        _ => {
            if raw.is_empty() {
                return None;
            }
            return Some(FilterValue::Str(raw.to_string()));
        }
    }
}

/// A coerced filter value as JSON (for `In`/`NotIn` operand lists).
pub fn value_to_json(value: &FilterValue) -> serde_json::Value {
    return match value {
        FilterValue::Null => serde_json::Value::Null,
        FilterValue::Bool(b) => serde_json::Value::Bool(*b),
        FilterValue::Int(n) => serde_json::Value::from(*n),
        FilterValue::Float(n) => serde_json::Value::from(*n),
        FilterValue::Str(s) => serde_json::Value::String(s.clone()),
        FilterValue::DateTime(dt) => serde_json::Value::String(dt.to_rfc3339()),
        FilterValue::In(_) => serde_json::Value::Null,
        FilterValue::Range { .. } => serde_json::Value::Null,
    };
}
