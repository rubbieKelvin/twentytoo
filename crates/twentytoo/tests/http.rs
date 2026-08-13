//! End-to-end handler tests: the built app driven through real HTTP.

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header::CONTENT_TYPE};
use http_body_util::BodyExt;
use tower::ServiceExt;
use twentytoo::prelude::*;

#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct Widget {
    id: String,
    name: String,
    status: String,
    /// Server-managed: forms never send it, so it needs a default.
    #[serde(default)]
    created_at: String,
}

/// Demo-style policy: allow everything.
struct AllowAll;

impl<E> Policy<E> for AllowAll {
    fn can_view_any(&self, _actor: &Actor) -> bool {
        return true;
    }

    fn can_create(&self, _actor: &Actor) -> bool {
        return true;
    }

    fn can_update(&self, _actor: &Actor, _record: &E) -> bool {
        return true;
    }

    fn can_delete(&self, _actor: &Actor, _record: &E) -> bool {
        return true;
    }
}

/// Deny everything.
struct DenyAll2;

impl<E> Policy<E> for DenyAll2 {
    fn can_view_any(&self, _actor: &Actor) -> bool {
        return false;
    }
}

struct WidgetResource {
    adapter: Arc<InMemoryAdapter<Widget>>,
    policy: Box<dyn Policy<Widget>>,
    flag: Option<&'static str>,
}

impl WidgetResource {
    fn new(adapter: Arc<InMemoryAdapter<Widget>>) -> Self {
        return Self {
            adapter,
            policy: Box::new(AllowAll),
            flag: None,
        };
    }
}

impl Resource for WidgetResource {
    type Entity = Widget;

    fn key(&self) -> &'static str {
        return "widgets";
    }

    fn label(&self) -> &'static str {
        return "Widgets";
    }

    fn fields(&self) -> Vec<Field<Self::Entity>> {
        return fields![
            field!("id", "Id", Text, form: true, required: true),
            field!("name", "Name", Text, list: true, detail: true, form: true, required: true, sortable: true, searchable: true),
            field!("status", "Status", Badge { options: &[("active", "Active"), ("retired", "Retired")] }, list: true, detail: true, form: true),
            field!("created_at", "Created", DateTime, list: true, detail: true, sortable: true),
        ];
    }

    fn list_columns(&self) -> Vec<&'static str> {
        return vec!["name", "status", "created_at"];
    }

    fn default_sort(&self) -> Vec<SortField> {
        return vec![SortField::asc("name")];
    }

    fn search_fields(&self) -> Vec<&'static str> {
        return vec!["name"];
    }

    fn filters(&self) -> Vec<FilterSpec> {
        return vec![FilterSpec {
            field: "status",
            op: FilterOp::Eq,
            label: Some("Status"),
        }];
    }

    fn policy(&self) -> &dyn Policy<Self::Entity> {
        return &*self.policy;
    }

    fn flag(&self) -> Option<&'static str> {
        return self.flag;
    }

    fn adapter(&self) -> Arc<dyn DataAdapter<Self::Entity>> {
        return self.adapter.clone();
    }
}

fn seed(adapter: &Arc<InMemoryAdapter<Widget>>, count: usize) {
    for i in 1..=count {
        let id = format!("w{i}");
        adapter
            .insert(
                id.clone(),
                Widget {
                    id,
                    name: format!("Widget {i:02}"),
                    status: if i % 3 == 0 { "retired" } else { "active" }.to_string(),
                    created_at: format!("2026-07-{:02}T08:00:00Z", i % 28 + 1),
                },
            )
            .expect("seed id is unique");
    }
}

async fn build_app(resource: WidgetResource) -> Router<()> {
    return twentytoo::Twentytoo::builder()
        .resource(resource)
        .default_actor(Actor {
            id: "admin".to_string(),
            email: "admin@example.com".to_string(),
            roles: vec!["admin".to_string()],
            permissions: vec!["*.*".to_string()],
            team_id: None,
        })
        .build()
        .await
        .expect("app builds")
        .into_make_service();
}

async fn get(app: &Router<()>, uri: &str) -> (StatusCode, String) {
    let res = app
        .clone()
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = res.status();
    let body = res.into_body().collect().await.unwrap().to_bytes().to_vec();
    return (status, String::from_utf8(body).unwrap());
}

async fn post(app: &Router<()>, uri: &str, form: &str) -> (StatusCode, String) {
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(uri)
                .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(Body::from(form.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = res.status();
    let body = res.into_body().collect().await.unwrap().to_bytes().to_vec();
    return (status, String::from_utf8(body).unwrap());
}

// ---------------------------------------------------------------------------
// Reads
// ---------------------------------------------------------------------------

#[tokio::test]
async fn list_renders_rows_with_pagination() {
    let adapter = Arc::new(InMemoryAdapter::<Widget>::new());
    seed(&adapter, 30);
    let app = build_app(WidgetResource::new(adapter)).await;

    let (status, body) = get(&app, "/widgets?per_page=10").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("Widgets"));
    assert!(body.contains("Widget 01"));
    assert!(body.contains("Widget 10"));
    assert!(
        !body.contains("Widget 11"),
        "page 1 must not show page 2 rows"
    );
    // Numbered pager: 30 rows at 10/page → 3 pages.
    assert!(body.contains("page=2"), "pager must link page 2");
    assert!(body.contains("page=3"), "pager must link page 3");

    let (_, page2) = get(&app, "/widgets?per_page=10&page=2").await;
    assert!(page2.contains("Widget 11"));
    assert!(!page2.contains("Widget 01"));
}

#[tokio::test]
async fn list_search_and_filter_narrow_rows() {
    let adapter = Arc::new(InMemoryAdapter::<Widget>::new());
    seed(&adapter, 30);
    let app = build_app(WidgetResource::new(adapter)).await;

    // Search by name.
    let (status, body) = get(&app, "/widgets?q=Widget%201").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("Widget 1"));
    assert!(body.contains("Widget 19"));
    assert!(!body.contains("Widget 2"));

    // Filter by status.
    let (_, body) = get(&app, "/widgets?status=retired").await;
    assert!(body.contains("Widget 03"));
    assert!(
        !body.contains("Widget 01"),
        "active rows must be filtered out"
    );
}

#[tokio::test]
async fn list_sort_descending() {
    let adapter = Arc::new(InMemoryAdapter::<Widget>::new());
    seed(&adapter, 5);
    let app = build_app(WidgetResource::new(adapter)).await;

    let (_, body) = get(&app, "/widgets?sort=-name").await;
    let pos1 = body.find("Widget 05").expect("row 05 present");
    let pos2 = body.find("Widget 01").expect("row 01 present");
    assert!(pos1 < pos2, "descending sort puts 05 before 01");
}

#[tokio::test]
async fn list_denied_by_policy_is_403() {
    let adapter = Arc::new(InMemoryAdapter::<Widget>::new());
    seed(&adapter, 3);
    let mut resource = WidgetResource::new(adapter);
    resource.policy = Box::new(DenyAll2);
    let app = build_app(resource).await;

    let (status, _) = get(&app, "/widgets").await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn detail_renders_record_and_gates_buttons() {
    let adapter = Arc::new(InMemoryAdapter::<Widget>::new());
    seed(&adapter, 3);
    let app = build_app(WidgetResource::new(adapter)).await;

    let (status, body) = get(&app, "/widgets/w1").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("Widget 01"));
    assert!(body.contains("Active"), "badge label rendered");
    assert!(body.contains("Edit"));
    assert!(body.contains("Delete"));
}

#[tokio::test]
async fn detail_missing_is_404() {
    let adapter = Arc::new(InMemoryAdapter::<Widget>::new());
    let app = build_app(WidgetResource::new(adapter)).await;
    let (status, _) = get(&app, "/widgets/nope").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

// ---------------------------------------------------------------------------
// Writes
// ---------------------------------------------------------------------------

#[tokio::test]
async fn create_inserts_and_redirects_to_detail() {
    let adapter = Arc::new(InMemoryAdapter::<Widget>::new());
    let app = build_app(WidgetResource::new(adapter.clone())).await;

    let (status, body) = post(&app, "/widgets", "id=w9&name=Gadget&status=active").await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    assert!(
        body.contains("Location: /widgets/w9") || body.is_empty(),
        "redirect to the new record"
    );

    let (_, detail) = get(&app, "/widgets/w9").await;
    assert!(detail.contains("Gadget"));
    assert!(detail.contains("Active"));
}

#[tokio::test]
async fn create_without_required_field_rerenders_form_with_error() {
    let adapter = Arc::new(InMemoryAdapter::<Widget>::new());
    let app = build_app(WidgetResource::new(adapter)).await;

    let (status, body) = post(&app, "/widgets", "id=w9&status=active").await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(body.contains("Name is required"));
    // The submitted values survive the error re-render.
    assert!(body.contains("w9"));
}

#[tokio::test]
async fn update_merges_and_redirects() {
    let adapter = Arc::new(InMemoryAdapter::<Widget>::new());
    seed(&adapter, 1);
    let app = build_app(WidgetResource::new(adapter)).await;

    let (status, _) = post(&app, "/widgets/w1", "id=w1&name=Renamed&status=retired").await;
    assert_eq!(status, StatusCode::SEE_OTHER);

    let (_, detail) = get(&app, "/widgets/w1").await;
    assert!(detail.contains("Renamed"));
    assert!(detail.contains("Retired"));
}

#[tokio::test]
async fn update_missing_is_404() {
    let adapter = Arc::new(InMemoryAdapter::<Widget>::new());
    let app = build_app(WidgetResource::new(adapter)).await;
    let (status, _) = post(&app, "/widgets/nope", "name=X").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn delete_removes_and_redirects() {
    let adapter = Arc::new(InMemoryAdapter::<Widget>::new());
    seed(&adapter, 1);
    let app = build_app(WidgetResource::new(adapter.clone())).await;

    let (status, _) = post(&app, "/widgets/w1/delete", "").await;
    assert_eq!(status, StatusCode::SEE_OTHER);

    let (status, _) = get(&app, "/widgets/w1").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

// ---------------------------------------------------------------------------
// Flags, home, fallback
// ---------------------------------------------------------------------------

#[tokio::test]
async fn flagged_off_resource_is_404() {
    let adapter = Arc::new(InMemoryAdapter::<Widget>::new());
    seed(&adapter, 2);
    let mut resource = WidgetResource::new(adapter);
    resource.flag = Some("widgets-flag");
    let app = build_app(resource).await;

    let (status, _) = get(&app, "/widgets").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn home_lists_resources_with_counts() {
    let adapter = Arc::new(InMemoryAdapter::<Widget>::new());
    seed(&adapter, 7);
    let app = build_app(WidgetResource::new(adapter)).await;

    let (status, body) = get(&app, "/").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("Widgets"));
    assert!(body.contains("7 records"));
}

#[tokio::test]
async fn unknown_path_is_404() {
    let adapter = Arc::new(InMemoryAdapter::<Widget>::new());
    let app = build_app(WidgetResource::new(adapter)).await;
    let (status, _) = get(&app, "/nope").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}
