//! The resource registry: erased per-resource facts for nav and home.

use std::sync::Arc;

use async_trait::async_trait;
use serde::Serialize;
use twentytoo_core::{Actor, DataError, Pagination, Query, Resource};

/// One nav entry for the sidebar (`01-ui-kit` §6).
#[derive(Clone, Debug, Serialize)]
pub struct NavItem {
    /// Resource key (`"stores"`).
    pub key: &'static str,
    /// Human label (`"Stores"`).
    pub label: &'static str,
    /// Icon name from the closed set (`01-ui-kit` §7.13).
    pub icon: &'static str,
}

/// One home-page card.
#[derive(Clone, Debug, Serialize)]
pub struct HomeCard {
    /// Resource key.
    pub key: &'static str,
    /// Human label.
    pub label: &'static str,
    /// Icon name from the closed set (`01-ui-kit` §7.13).
    pub icon: &'static str,
    /// Record count, when the source can count cheaply (`Page.total`).
    pub count: Option<u64>,
}

/// Erased per-resource facts the non-generic handlers (home, nav) need.
#[async_trait]
pub trait DynResourceMeta: Send + Sync {
    /// Resource key.
    fn key(&self) -> &'static str;
    /// Human label.
    fn label(&self) -> &'static str;
    /// Icon name from the closed set (`01-ui-kit` §7.13).
    fn icon(&self) -> &'static str;
    /// `policy().can_view_any(actor)`.
    fn can_view_any(&self, actor: &Actor) -> bool;
    /// `policy().can_create(actor)`.
    fn can_create(&self, actor: &Actor) -> bool;
    /// Cheap record count: one list call for `Page.total`.
    async fn count(&self) -> Result<Option<u64>, DataError>;
}

/// The concrete, generic-backed meta for one resource.
pub struct ResourceMeta<R: Resource> {
    /// The registered resource.
    pub resource: Arc<R>,
}

#[async_trait]
#[allow(clippy::implicit_return)]
impl<R: Resource> DynResourceMeta for ResourceMeta<R> {
    fn key(&self) -> &'static str {
        return self.resource.key();
    }

    fn label(&self) -> &'static str {
        return self.resource.label();
    }

    fn icon(&self) -> &'static str {
        return self.resource.icon();
    }

    fn can_view_any(&self, actor: &Actor) -> bool {
        return self.resource.policy().can_view_any(actor);
    }

    fn can_create(&self, actor: &Actor) -> bool {
        return self.resource.policy().can_create(actor);
    }

    async fn count(&self) -> Result<Option<u64>, DataError> {
        let page = self
            .resource
            .adapter()
            .list(&Query {
                pagination: Pagination::Offset {
                    page: 1,
                    per_page: 1,
                },
                sort: Vec::new(),
                filter: None,
                search: None,
                projection: None,
            })
            .await?;
        return Ok(page.total);
    }
}

/// One erased meta per registered resource.
pub struct ResourceRegistry {
    metas: Vec<Box<dyn DynResourceMeta>>,
}

impl ResourceRegistry {
    /// Wrap the collected metas.
    pub fn new(metas: Vec<Box<dyn DynResourceMeta>>) -> Self {
        return Self { metas };
    }

    /// Nav entries, in registration order.
    pub fn nav(&self) -> Vec<NavItem> {
        return self
            .metas
            .iter()
            .map(|m| {
                return NavItem {
                    key: m.key(),
                    label: m.label(),
                    icon: m.icon(),
                };
            })
            .collect();
    }

    /// Home cards for `actor`: only resources the actor may view, with
    /// counts filled by one cheap list call each.
    pub async fn home_cards(&self, actor: &Actor) -> Vec<HomeCard> {
        let mut cards: Vec<HomeCard> = self
            .metas
            .iter()
            .filter(|m| return m.can_view_any(actor))
            .map(|m| {
                return HomeCard {
                    key: m.key(),
                    label: m.label(),
                    icon: m.icon(),
                    count: None,
                };
            })
            .collect();
        for (card, meta) in cards.iter_mut().zip(&self.metas) {
            card.count = meta.count().await.ok().flatten();
        }
        return cards;
    }
}
