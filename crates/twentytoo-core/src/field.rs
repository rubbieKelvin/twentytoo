//! Field metadata: how a resource's entity is rendered and validated.

use std::marker::PhantomData;

/// The rendering/validation kind of a field.
///
/// Non-generic by design: it describes serialized JSON entities, so
/// `FieldSpec`, `ActionField`, and the view layer all share it without an
/// entity type parameter.
///
/// `Computed` carries a function pointer, so derived equality on that
/// variant is degenerate (address comparison) — acceptable for a metadata
/// type; the lint is deliberate.
#[allow(unpredictable_function_pointer_comparisons)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FieldKind {
    /// Single-line text.
    Text,
    /// Multi-line text.
    Textarea,
    /// Rich (HTML) text.
    Richtext,
    /// Numeric value.
    Number,
    /// Currency value.
    Currency,
    /// Checkbox.
    Boolean,
    /// Single pick from options (value, label).
    Select {
        /// (value, label) pairs.
        options: Vec<(&'static str, &'static str)>,
    },
    /// Multiple picks from options.
    MultiSelect {
        /// (value, label) pairs.
        options: Vec<(&'static str, &'static str)>,
    },
    /// A calendar date.
    Date,
    /// A date-time.
    DateTime,
    /// An email address.
    Email,
    /// A file upload; `accept` is the input's accept hint.
    File {
        /// Accepted MIME/extension hint.
        accept: Option<&'static str>,
    },
    /// An image upload.
    Image {
        /// Accepted MIME/extension hint.
        accept: Option<&'static str>,
    },
    /// A reference to another resource.
    Relation {
        /// The related resource's key.
        resource_key: &'static str,
        /// Field on the related entity to display.
        display_field: &'static str,
    },
    /// A colored status chip.
    Badge {
        /// (value, label) pairs.
        options: Vec<(&'static str, &'static str)>,
    },
    /// Raw JSON.
    Json,
    /// Rendered from the serialized entity.
    Computed {
        /// Render the serialized entity to display text.
        render: fn(&serde_json::Value) -> String,
    },
}

/// One field of a resource's entity.
///
/// `validator` runs over the typed entity. `visible_to`/`editable_by` are
/// permission lists in `"resource.action"` form; `flag` gates the field
/// behind a feature flag.
#[derive(Clone, Debug)]
pub struct Field<E> {
    /// Machine name (`"status"`).
    pub name: &'static str,
    /// Human label (`"Status"`).
    pub label: &'static str,
    /// Rendering kind.
    pub kind: FieldKind,
    /// Show in list views.
    pub show_in_list: bool,
    /// Show in detail views.
    pub show_in_detail: bool,
    /// Show in forms.
    pub show_in_form: bool,
    /// Required in forms.
    pub required: bool,
    /// Sortable in list views.
    pub sortable: bool,
    /// Included in search.
    pub searchable: bool,
    /// Roles that may see the field.
    pub visible_to: Vec<&'static str>,
    /// Roles that may edit the field.
    pub editable_by: Vec<&'static str>,
    /// Feature flag gating this field.
    pub flag: Option<&'static str>,
    /// Entity-level validator.
    #[allow(clippy::type_complexity)] // plan-mandated `fn` pointer signature
    pub validator: Option<fn(&E) -> Result<(), String>>,
    /// Type anchor; `Field<E>` carries no `E` data.
    pub _marker: PhantomData<E>,
}

// All `Field` fields are bounds-free, so equality must not demand `E:
// PartialEq`.
impl<E> PartialEq for Field<E> {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
            && self.label == other.label
            && self.kind == other.kind
            && self.show_in_list == other.show_in_list
            && self.show_in_detail == other.show_in_detail
            && self.show_in_form == other.show_in_form
            && self.required == other.required
            && self.sortable == other.sortable
            && self.searchable == other.searchable
            && self.visible_to == other.visible_to
            && self.editable_by == other.editable_by
            && self.flag == other.flag
    }
}

impl<E> Eq for Field<E> {}

/// A discovered source column, from `DataAdapter::describe`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FieldSpec {
    /// Column / mapping / sample-document key.
    pub name: String,
    /// Inferred kind (best-effort; the resource definition is authoritative).
    pub kind: FieldKind,
    /// Whether the source allows null.
    pub nullable: bool,
}

/// Build a field vector from field expressions.
#[macro_export]
macro_rules! fields {
    ($($field:expr),* $(,)?) => {
        vec![$($field),*]
    };
}

/// Build one [`Field`](crate::field::Field) concisely.
///
/// Flags (`required`, `list`, `detail`, `form`, `sortable`, `searchable`)
/// may appear in any order, each `: true` or `: false`. The kind argument is
/// either a bare ident (`Text`, `Number`, …) or a braced form (`Select {
/// options: &[…] }`, `Relation { resource_key, display_field }`,
/// `Computed { render: path }`, `File { accept: "…" }`).
///
/// `visible_to`, `editable_by`, `flag`, and `validator` are not macro flags;
/// set them with struct-update syntax:
/// `Field { visible_to: vec!["admin"], ..field!(…) }`.
#[macro_export]
macro_rules! field {
    ($name:expr, $label:expr, $kind:ident $( { $($inner:tt)* } )? $(, $flag:ident : $value:expr)* $(,)?) => {{
        let kind = $crate::field_kind!($kind $( { $($inner)* } )?);
        let mut __field = $crate::field::Field {
            name: $name,
            label: $label,
            kind,
            show_in_list: false,
            show_in_detail: false,
            show_in_form: false,
            required: false,
            sortable: false,
            searchable: false,
            visible_to: vec![],
            editable_by: vec![],
            flag: None,
            validator: None,
            _marker: ::std::marker::PhantomData,
        };
        $(
            $crate::__set_flag!(&mut __field, $flag, $value);
        )*
        __field
    }};
}

/// Internal: set one `field!` flag on a `Field`.
#[doc(hidden)]
#[macro_export]
macro_rules! __set_flag {
    ($f:expr, required, $v:expr) => {
        $f.required = $v;
    };
    ($f:expr, list, $v:expr) => {
        $f.show_in_list = $v;
    };
    ($f:expr, detail, $v:expr) => {
        $f.show_in_detail = $v;
    };
    ($f:expr, form, $v:expr) => {
        $f.show_in_form = $v;
    };
    ($f:expr, sortable, $v:expr) => {
        $f.sortable = $v;
    };
    ($f:expr, searchable, $v:expr) => {
        $f.searchable = $v;
    };
}

/// Internal: expand a `field!` kind argument to a `FieldKind`.
#[doc(hidden)]
#[macro_export]
macro_rules! field_kind {
    (Text) => {
        $crate::field::FieldKind::Text
    };
    (Textarea) => {
        $crate::field::FieldKind::Textarea
    };
    (Richtext) => {
        $crate::field::FieldKind::Richtext
    };
    (Number) => {
        $crate::field::FieldKind::Number
    };
    (Currency) => {
        $crate::field::FieldKind::Currency
    };
    (Boolean) => {
        $crate::field::FieldKind::Boolean
    };
    (Date) => {
        $crate::field::FieldKind::Date
    };
    (DateTime) => {
        $crate::field::FieldKind::DateTime
    };
    (Email) => {
        $crate::field::FieldKind::Email
    };
    (Json) => {
        $crate::field::FieldKind::Json
    };
    (Select { options: $opts:expr }) => {
        $crate::field::FieldKind::Select {
            options: Vec::from($opts),
        }
    };
    (MultiSelect { options: $opts:expr }) => {
        $crate::field::FieldKind::MultiSelect {
            options: Vec::from($opts),
        }
    };
    (Badge { options: $opts:expr }) => {
        $crate::field::FieldKind::Badge {
            options: Vec::from($opts),
        }
    };
    (File { accept: $accept:expr }) => {
        $crate::field::FieldKind::File {
            accept: Some($accept),
        }
    };
    (File) => {
        $crate::field::FieldKind::File { accept: None }
    };
    (File {}) => {
        $crate::field::FieldKind::File { accept: None }
    };
    (Image { accept: $accept:expr }) => {
        $crate::field::FieldKind::Image {
            accept: Some($accept),
        }
    };
    (Image) => {
        $crate::field::FieldKind::Image { accept: None }
    };
    (Image {}) => {
        $crate::field::FieldKind::Image { accept: None }
    };
    (Relation { resource_key: $key:expr, display_field: $display:expr }) => {
        $crate::field::FieldKind::Relation {
            resource_key: $key,
            display_field: $display,
        }
    };
    (Computed { render: $render:expr }) => {
        $crate::field::FieldKind::Computed { render: $render }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Project `.kind`, pinning `E` (the macro leaves it unconstrained).
    fn kind(f: Field<serde_json::Value>) -> FieldKind {
        f.kind
    }

    fn render_name(v: &serde_json::Value) -> String {
        format!("name={}", v["name"].as_str().unwrap_or_default())
    }

    #[test]
    fn bare_idents_map_to_variants() {
        assert_eq!(kind(field!("a", "A", Text)), FieldKind::Text);
        assert_eq!(kind(field!("b", "B", Textarea)), FieldKind::Textarea);
        assert_eq!(kind(field!("c", "C", Richtext)), FieldKind::Richtext);
        assert_eq!(kind(field!("d", "D", Number)), FieldKind::Number);
        assert_eq!(kind(field!("e", "E", Currency)), FieldKind::Currency);
        assert_eq!(kind(field!("f", "F", Boolean)), FieldKind::Boolean);
        assert_eq!(kind(field!("g", "G", Date)), FieldKind::Date);
        assert_eq!(kind(field!("h", "H", DateTime)), FieldKind::DateTime);
        assert_eq!(kind(field!("i", "I", Email)), FieldKind::Email);
        assert_eq!(kind(field!("j", "J", Json)), FieldKind::Json);
    }

    #[test]
    fn select_converts_slice_to_vec() {
        let f: Field<serde_json::Value> = field!(
            "status",
            "Status",
            Select {
                options: &[("open", "Open"), ("done", "Done")]
            }
        );
        assert_eq!(
            f.kind,
            FieldKind::Select {
                options: vec![("open", "Open"), ("done", "Done")]
            }
        );
    }

    #[test]
    fn multi_select_and_badge() {
        let f: Field<serde_json::Value> = field!(
            "tags",
            "Tags",
            MultiSelect {
                options: &[("a", "A")]
            }
        );
        assert_eq!(
            f.kind,
            FieldKind::MultiSelect {
                options: vec![("a", "A")]
            }
        );
        let b: Field<serde_json::Value> = field!(
            "status",
            "Status",
            Badge {
                options: &[("x", "X")]
            }
        );
        assert_eq!(
            b.kind,
            FieldKind::Badge {
                options: vec![("x", "X")]
            }
        );
    }

    #[test]
    fn file_and_image_accept() {
        assert_eq!(
            kind(field!("doc", "Doc", File { accept: ".pdf" })),
            FieldKind::File {
                accept: Some(".pdf")
            }
        );
        assert_eq!(
            kind(field!("doc", "Doc", File)),
            FieldKind::File { accept: None }
        );
        assert_eq!(
            kind(field!("doc", "Doc", File {})),
            FieldKind::File { accept: None }
        );
        assert_eq!(
            kind(field!(
                "img",
                "Img",
                Image {
                    accept: "image/png"
                }
            )),
            FieldKind::Image {
                accept: Some("image/png")
            }
        );
        assert_eq!(
            kind(field!("img", "Img", Image)),
            FieldKind::Image { accept: None }
        );
    }

    #[test]
    fn relation_and_computed() {
        let rel: Field<serde_json::Value> = field!(
            "store",
            "Store",
            Relation {
                resource_key: "stores",
                display_field: "name"
            }
        );
        assert_eq!(
            rel.kind,
            FieldKind::Relation {
                resource_key: "stores",
                display_field: "name"
            }
        );
        let comp: Field<serde_json::Value> = field!(
            "name",
            "Name",
            Computed {
                render: render_name
            }
        );
        let FieldKind::Computed { render } = comp.kind else {
            panic!("expected Computed kind");
        };
        assert_eq!(render(&serde_json::json!({"name": "Seven"})), "name=Seven");
    }

    #[test]
    fn flags_land_on_the_right_bools() {
        let f: Field<serde_json::Value> = field!(
            "status",
            "Status",
            Text,
            required: true,
            list: true,
            searchable: true,
            detail: false,
            form: true,
            sortable: false
        );
        assert!(f.required);
        assert!(f.show_in_list);
        assert!(f.searchable);
        assert!(!f.show_in_detail);
        assert!(f.show_in_form);
        assert!(!f.sortable);
    }

    #[test]
    fn unspecified_flags_default_false() {
        let f: Field<serde_json::Value> = field!("status", "Status", Text);
        assert!(!f.required);
        assert!(!f.show_in_list);
        assert!(!f.show_in_detail);
        assert!(!f.show_in_form);
        assert!(!f.sortable);
        assert!(!f.searchable);
        assert!(f.visible_to.is_empty());
        assert!(f.editable_by.is_empty());
        assert_eq!(f.flag, None);
        assert!(f.validator.is_none());
    }

    #[test]
    fn fields_accepts_trailing_comma() {
        let fs: Vec<Field<serde_json::Value>> =
            fields![field!("a", "A", Text), field!("b", "B", Number, list: true),];
        assert_eq!(fs.len(), 2);
        assert_eq!(fs[0].name, "a");
        assert_eq!(fs[1].kind, FieldKind::Number);
    }

    #[test]
    fn struct_update_sets_deferred_fields() {
        let f: Field<serde_json::Value> = Field {
            visible_to: vec!["admin"],
            ..field!("status", "Status", Text)
        };
        assert_eq!(f.visible_to, vec!["admin"]);
        assert_eq!(f.name, "status");
    }
}
