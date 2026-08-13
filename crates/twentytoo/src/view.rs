//! View models: the serializable per-resource render context.
//!
//! The view layer touches entities only as serialized JSON (`03` §3.1);
//! these models are what the templates actually see. Field visibility
//! (`visible_to`), editability (`editable_by`), and policy gates are
//! applied here, per actor, before anything reaches a template.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use twentytoo_core::{Actor, Field, FieldKind, FilterOp, Resource};

/// One option of a `Select`/`Badge`/`MultiSelect` kind.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChoiceView {
    /// Stored value.
    pub value: String,
    /// Display label.
    pub label: String,
}

/// A `FieldKind` as the templates can see it: a tag plus option list.
///
/// `FieldKind::Computed`'s render function never serializes — computed
/// values are materialized into the row by the handler instead, and the
/// kind arrives here as the plain `"computed"` tag.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct KindView {
    /// Kind tag (`"text"`, `"select"`, `"badge"`, `"currency"`, …).
    pub tag: String,
    /// Options for select-like kinds.
    pub options: Vec<ChoiceView>,
    /// Related resource's key, for `relation` kinds.
    pub relation: Option<String>,
}

impl KindView {
    /// Map a core `FieldKind` to its template view.
    pub fn of(kind: &FieldKind) -> Self {
        let (tag, options, relation) = match kind {
            FieldKind::Text => ("text", Vec::new(), None),
            FieldKind::Textarea => ("textarea", Vec::new(), None),
            FieldKind::Richtext => ("richtext", Vec::new(), None),
            FieldKind::Number => ("number", Vec::new(), None),
            FieldKind::Currency => ("currency", Vec::new(), None),
            FieldKind::Boolean => ("boolean", Vec::new(), None),
            FieldKind::Date => ("date", Vec::new(), None),
            FieldKind::DateTime => ("datetime", Vec::new(), None),
            FieldKind::Email => ("email", Vec::new(), None),
            FieldKind::Json => ("json", Vec::new(), None),
            FieldKind::File { .. } => ("file", Vec::new(), None),
            FieldKind::Image { .. } => ("image", Vec::new(), None),
            FieldKind::Relation { resource_key, .. } => {
                ("relation", Vec::new(), Some((*resource_key).to_string()))
            }
            FieldKind::Computed { .. } => ("computed", Vec::new(), None),
            FieldKind::Select { options }
            | FieldKind::MultiSelect { options }
            | FieldKind::Badge { options } => {
                let tag = match kind {
                    FieldKind::Select { .. } => "select",
                    FieldKind::MultiSelect { .. } => "multiselect",
                    _ => "badge",
                };
                let choices = options
                    .iter()
                    .map(|(value, label)| {
                        return ChoiceView {
                            value: (*value).to_string(),
                            label: (*label).to_string(),
                        };
                    })
                    .collect();
                (tag, choices, None)
            }
        };
        return Self {
            tag: tag.to_string(),
            options,
            relation,
        };
    }
}

/// One field as templates see it.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FieldView {
    /// Machine name (`"status"`).
    pub name: String,
    /// Human label (`"Status"`).
    pub label: String,
    /// Rendering kind.
    pub kind: KindView,
    /// Required in forms.
    pub required: bool,
    /// Sortable in list views.
    pub sortable: bool,
}

/// One filter as templates see it.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FilterView {
    /// Field name — also the query-param name.
    pub name: String,
    /// Sidebar label.
    pub label: String,
    /// Field kind, for the control widget.
    pub kind: KindView,
    /// Operator tag (`"eq"`, `"contains"`, `"gt"`, …).
    pub op: String,
    /// The active value from the request, if any.
    pub current: Option<String>,
}

/// One page link of the numbered pager.
#[derive(Clone, Debug, Serialize)]
pub struct PageLink {
    /// Page number.
    pub page: usize,
    /// Prebuilt URL (path + preserved params).
    pub url: String,
}

/// The pager: exactly one of two modes (`03` §4.3).
#[derive(Clone, Debug, Serialize)]
pub struct PagerView {
    /// `"numbered"` when the source counts cheaply, else `"prevnext"`.
    pub mode: &'static str,
    /// Current page (1-based).
    pub current: usize,
    /// Total pages (`numbered` mode only).
    pub total_pages: Option<usize>,
    /// Windowed page links (`numbered` mode).
    pub page_links: Vec<PageLink>,
    /// Previous-page URL, when one exists.
    pub prev_url: Option<String>,
    /// Next-page URL, when one exists.
    pub next_url: Option<String>,
}

/// One resource's render context.
#[derive(Clone, Debug, Serialize)]
pub struct ResourceView {
    /// Resource key (`"stores"`).
    pub key: String,
    /// Human label.
    pub label: String,
    /// List columns (visible + `show_in_list`).
    pub columns: Vec<FieldView>,
    /// Detail rows (visible + `show_in_detail`).
    pub detail_fields: Vec<FieldView>,
    /// Form fields (visible + `editable_by` + `show_in_form`; no
    /// file/image kinds — uploads land in a later slice).
    pub form_fields: Vec<FieldView>,
    /// Sidebar filters the source can express.
    pub filters: Vec<FilterView>,
    /// Any sort at all (`Capabilities.sort`).
    pub sortable: bool,
    /// Search box shown (`Capabilities.search` + `search_fields`).
    pub searchable: bool,
}

impl ResourceView {
    /// Build the view for `actor`, applying field visibility and the
    /// adapter's capabilities.
    pub fn for_actor<R: Resource>(resource: &R, actor: &Actor) -> Self {
        let caps = resource.adapter().capabilities();
        let fields = resource.fields();
        let by_name: HashMap<&str, &Field<R::Entity>> = fields
            .iter()
            .map(|f| {
                return (f.name, f);
            })
            .collect();

        let visible = visible_fields(&fields, actor);
        let editable = |f: &Field<R::Entity>| -> bool {
            return f.editable_by.is_empty()
                || f.editable_by.iter().any(|r| return actor.has_role(r));
        };

        let columns = resource
            .list_columns()
            .iter()
            .filter_map(|name| {
                let f = by_name.get(name)?;
                if !f.show_in_list || !visible.iter().any(|v| return v.name == *name) {
                    return None;
                }
                return Some(FieldView::of(f));
            })
            .collect();

        let detail_fields = visible
            .iter()
            .filter(|f| return f.show_in_detail)
            .map(|f| return FieldView::of(f))
            .collect();

        let form_fields = visible
            .iter()
            .filter(|f| {
                return f.show_in_form
                    && editable(f)
                    && !matches!(f.kind, FieldKind::File { .. } | FieldKind::Image { .. });
            })
            .map(|f| return FieldView::of(f))
            .collect();

        let filters = resource
            .filters()
            .iter()
            .filter(|spec| return caps.filter_ops.contains(&spec.op))
            .filter_map(|spec| {
                let f = by_name.get(spec.field)?;
                return Some(FilterView {
                    name: spec.field.to_string(),
                    label: spec.label.unwrap_or(f.label).to_string(),
                    kind: KindView::of(&f.kind),
                    op: op_tag(spec.op).to_string(),
                    current: None,
                });
            })
            .collect();

        let searchable = !matches!(caps.search, twentytoo_core::SearchMode::None)
            && !resource.search_fields().is_empty();

        return Self {
            key: resource.key().to_string(),
            label: resource.label().to_string(),
            columns,
            detail_fields,
            form_fields,
            filters,
            sortable: caps.sort,
            searchable,
        };
    }

    /// Attach the request's current filter values to the filter views.
    pub fn with_filter_values(mut self, params: &HashMap<String, String>) -> Self {
        for f in &mut self.filters {
            f.current = params.get(&f.name).cloned();
        }
        return self;
    }
}

impl FieldView {
    /// Build a field view from a core `Field`.
    fn of<E>(field: &Field<E>) -> Self {
        return Self {
            name: field.name.to_string(),
            label: field.label.to_string(),
            kind: KindView::of(&field.kind),
            required: field.required,
            sortable: field.sortable,
        };
    }
}

/// The template tag for a filter operator.
fn op_tag(op: FilterOp) -> &'static str {
    match op {
        FilterOp::Eq => return "eq",
        FilterOp::Ne => return "ne",
        FilterOp::Gt => return "gt",
        FilterOp::Gte => return "gte",
        FilterOp::Lt => return "lt",
        FilterOp::Lte => return "lte",
        FilterOp::In => return "in",
        FilterOp::NotIn => return "notin",
        FilterOp::Contains => return "contains",
        FilterOp::StartsWith => return "startswith",
        FilterOp::IsNull => return "isnull",
        FilterOp::IsNotNull => return "isnotnull",
        FilterOp::FullText => return "fulltext",
    }
}

/// Fields visible to `actor`: `visible_to` empty, or containing one of the
/// actor's roles.
pub fn visible_fields<'a, E>(fields: &'a [Field<E>], actor: &Actor) -> Vec<&'a Field<E>> {
    return fields
        .iter()
        .filter(|f| {
            return f.visible_to.is_empty()
                || f.visible_to.iter().any(|r| return actor.has_role(r));
        })
        .collect();
}

/// Insert computed field values into a serialized row.
///
/// `Computed` kinds carry a render function that runs over the whole
/// record; the view layer serializes entities, so the handler materializes
/// computed columns into the row before rendering.
pub fn materialize_computed<E>(fields: &[Field<E>], row: &mut serde_json::Value) {
    for f in fields {
        if let FieldKind::Computed { render } = &f.kind {
            let text = serde_json::Value::String(render(row));
            if let serde_json::Value::Object(map) = row {
                map.insert(f.name.to_string(), text);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use twentytoo_core::{field, fields};

    #[test]
    fn kind_view_maps_select_options() {
        let kind = KindView::of(&FieldKind::Select {
            options: vec![("a", "A"), ("b", "B")],
        });
        assert_eq!(kind.tag, "select");
        assert_eq!(kind.options.len(), 2);
        assert_eq!(kind.options[0].value, "a");
        assert_eq!(kind.options[0].label, "A");
        assert!(kind.relation.is_none());
    }

    #[test]
    fn kind_view_carries_relation_key() {
        let kind = KindView::of(&FieldKind::Relation {
            resource_key: "stores",
            display_field: "name",
        });
        assert_eq!(kind.tag, "relation");
        assert_eq!(kind.relation.as_deref(), Some("stores"));
    }

    #[test]
    fn materialize_computed_inserts_rendered_value() {
        let f: Field<serde_json::Value> = field!(
            "slug",
            "Slug",
            Computed { render: |row| return format!("s-{}", row["id"].as_str().unwrap_or_default()) },
            list: true
        );
        let mut row = serde_json::json!({ "id": "7" });
        materialize_computed(&[f], &mut row);
        assert_eq!(row["slug"], "s-7");
    }

    #[test]
    fn visible_fields_respects_visible_to() {
        let f: Vec<Field<serde_json::Value>> = fields![
            field!("a", "A", Text, list: true),
            Field {
                visible_to: vec!["admin"],
                ..field!("b", "B", Text, list: true)
            },
            Field {
                visible_to: vec!["admin", "ops"],
                ..field!("c", "C", Text, list: true)
            },
        ];
        let viewer = Actor {
            id: "u".into(),
            email: "u@example.com".into(),
            roles: vec!["ops".into()],
            permissions: vec![],
            team_id: None,
        };
        let visible = visible_fields(&f, &viewer);
        let names: Vec<&str> = visible.iter().map(|v| return v.name).collect();
        assert_eq!(names, ["a", "c"]);
    }
}
