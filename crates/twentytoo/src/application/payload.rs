//! Form payloads → record JSON, with field-level validation.

use std::collections::HashMap;

use serde_json::{Map, Value};
use twentytoo_core::{Field, FieldKind, Resource};

/// Field-level validation errors, keyed by field name.
pub type FieldErrors = HashMap<String, String>;

/// Build the record payload from form values.
///
/// Rules, per kind:
/// - `Boolean`: absent (unchecked) → `false`; required means "must check".
/// - `MultiSelect`: all submitted values collected.
/// - `Number`/`Currency`: must parse; integers stay integers.
/// - `Json`: must parse as JSON.
/// - everything else: single string; empty optional values are omitted.
/// - `File`/`Image` never appear — the view model excludes them from forms.
pub fn build_payload<E>(
    fields: &[Field<E>],
    form: &HashMap<String, Vec<String>>,
) -> Result<Value, FieldErrors> {
    let mut errors = FieldErrors::new();
    let mut obj = Map::new();

    for field in fields {
        if !field.show_in_form {
            continue;
        }
        let name = field.name;
        let raw = form.get(name).map(|v| return v.as_slice()).unwrap_or(&[]);

        match &field.kind {
            FieldKind::Boolean => {
                if raw.is_empty() {
                    if field.required {
                        errors.insert(name.to_string(), format!("{} is required", field.label));
                    }
                    obj.insert(name.to_string(), Value::Bool(false));
                } else {
                    obj.insert(name.to_string(), Value::Bool(true));
                }
            }
            FieldKind::MultiSelect { .. } => {
                if raw.is_empty() {
                    if field.required {
                        errors.insert(name.to_string(), format!("{} is required", field.label));
                    }
                } else {
                    obj.insert(
                        name.to_string(),
                        Value::Array(
                            raw.iter()
                                .map(|s| return Value::String(s.clone()))
                                .collect(),
                        ),
                    );
                }
            }
            FieldKind::Number | FieldKind::Currency => match raw.first() {
                None => {
                    if field.required {
                        errors.insert(name.to_string(), format!("{} is required", field.label));
                    }
                }
                Some(s) if s.trim().is_empty() => {
                    if field.required {
                        errors.insert(name.to_string(), format!("{} is required", field.label));
                    }
                }
                Some(s) => match s.trim().parse::<f64>() {
                    Ok(n) if n.fract() == 0.0 && n.abs() < 9.0e15 => {
                        obj.insert(name.to_string(), Value::from(n as i64));
                    }
                    Ok(n) => {
                        obj.insert(name.to_string(), Value::from(n));
                    }
                    Err(_) => {
                        errors.insert(
                            name.to_string(),
                            format!("{} must be a number", field.label),
                        );
                    }
                },
            },
            FieldKind::Json => match raw.first() {
                None => {
                    if field.required {
                        errors.insert(name.to_string(), format!("{} is required", field.label));
                    }
                }
                Some(s) if s.trim().is_empty() => {
                    if field.required {
                        errors.insert(name.to_string(), format!("{} is required", field.label));
                    }
                }
                Some(s) => match serde_json::from_str::<Value>(s.trim()) {
                    Ok(v) => {
                        obj.insert(name.to_string(), v);
                    }
                    Err(_) => {
                        errors.insert(
                            name.to_string(),
                            format!("{} must be valid JSON", field.label),
                        );
                    }
                },
            },
            _ => match raw.first() {
                None => {
                    if field.required {
                        errors.insert(name.to_string(), format!("{} is required", field.label));
                    }
                }
                Some(s) if s.trim().is_empty() => {
                    if field.required {
                        errors.insert(name.to_string(), format!("{} is required", field.label));
                    }
                }
                Some(s) => {
                    obj.insert(name.to_string(), Value::String(s.trim().to_string()));
                }
            },
        }
    }

    if errors.is_empty() {
        return Ok(Value::Object(obj));
    }
    return Err(errors);
}

/// Run every entity-level validator over the assembled entity.
///
/// Returns the joined messages of all failing validators, or `None`.
pub fn run_validators<E>(fields: &[Field<E>], entity: &E) -> Option<String> {
    let messages: Vec<String> = fields
        .iter()
        .filter_map(|f| {
            let validator = f.validator?;
            return Some((f.label, validator));
        })
        .filter_map(|(label, validator)| {
            return validator(entity)
                .err()
                .map(|msg| return format!("{label}: {msg}"));
        })
        .collect();
    if messages.is_empty() {
        return None;
    }
    return Some(messages.join("; "));
}

/// The submitted form as a values object for error re-renders: first value
/// per key, strings.
pub fn form_values(form: &HashMap<String, Vec<String>>) -> Value {
    let obj: Map<String, Value> = form
        .iter()
        .filter_map(|(k, v)| {
            let first = v.first()?;
            return Some((k.clone(), Value::String(first.clone())));
        })
        .collect();
    return Value::Object(obj);
}

/// Entity-level validation: JSON → typed entity → validators → back.
pub fn validate_entity<R: Resource>(
    fields: &[Field<R::Entity>],
    payload: &Value,
) -> Option<String> {
    let entity: R::Entity = match serde_json::from_value(payload.clone()) {
        Ok(e) => e,
        Err(e) => {
            return Some(format!("payload does not match the entity: {e}"));
        }
    };
    return run_validators(fields, &entity);
}

#[cfg(test)]
mod tests {
    use super::*;
    use twentytoo_core::{field, fields};

    fn sample_fields() -> Vec<Field<serde_json::Value>> {
        return fields![
            field!("name", "Name", Text, form: true, required: true),
            field!("age", "Age", Number, form: true),
            field!("role", "Role", Select { options: &[("admin", "Admin")] }, form: true),
            field!("tags", "Tags", MultiSelect { options: &[("a", "A"), ("b", "B")] }, form: true),
            field!("active", "Active", Boolean, form: true, required: true),
            field!("note", "Note", Textarea, form: true),
        ];
    }

    #[test]
    fn coerces_kinds_and_omits_empty_optional() {
        let form = HashMap::from([
            ("name".to_string(), vec![" Ada ".to_string()]),
            ("age".to_string(), vec!["42".to_string()]),
            ("role".to_string(), vec!["admin".to_string()]),
            ("tags".to_string(), vec!["a".to_string(), "b".to_string()]),
            ("active".to_string(), vec!["on".to_string()]),
        ]);
        let payload = build_payload(&sample_fields(), &form).unwrap();
        assert_eq!(payload["name"], "Ada");
        assert_eq!(payload["age"], 42);
        assert_eq!(payload["role"], "admin");
        assert_eq!(payload["tags"], serde_json::json!(["a", "b"]));
        assert_eq!(payload["active"], true);
        assert!(payload.get("note").is_none());
    }

    #[test]
    fn unchecked_required_checkbox_fails() {
        let form = HashMap::from([("name".to_string(), vec!["x".to_string()])]);
        let errs = build_payload(&sample_fields(), &form).unwrap_err();
        assert!(errs["active"].contains("required"));
    }

    #[test]
    fn missing_required_text_fails() {
        let form = HashMap::new();
        let errs = build_payload(&sample_fields(), &form).unwrap_err();
        assert!(errs["name"].contains("required"));
    }

    #[test]
    fn bad_number_fails_with_label() {
        let form = HashMap::from([
            ("name".to_string(), vec!["x".to_string()]),
            ("age".to_string(), vec!["old".to_string()]),
        ]);
        let errs = build_payload(&sample_fields(), &form).unwrap_err();
        assert!(errs["age"].contains("number"));
    }

    #[test]
    fn form_values_takes_first_value() {
        let form = HashMap::from([
            ("tags".to_string(), vec!["a".to_string(), "b".to_string()]),
            ("name".to_string(), vec!["x".to_string()]),
        ]);
        assert_eq!(
            form_values(&form),
            serde_json::json!({"tags": "a", "name": "x"})
        );
    }
}
