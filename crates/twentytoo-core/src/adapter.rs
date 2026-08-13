//! The v2 data-source contract:
//! a graded trait every source can implement.

use async_trait::async_trait;
use futures::stream::BoxStream;
use serde::{Serialize, de::DeserializeOwned};

use crate::aggregation::{Aggregation, AggregationResult};
use crate::capabilities::Capabilities;
use crate::field::FieldSpec;
use crate::query::{Page, Pagination, Query};
use crate::write::{Mutation, WriteContext};

/// A data source behind one or more resources.
///
/// Graded contract: implement `capabilities`, `list`, `get`; everything else
/// has a default. The grade is declared, not discovered by trial.
///
/// `Id` is the source's id type (`String` for most, integers for SQL rows).
/// It is a defaulted generic — not an associated type — so `dyn DataAdapter<E>`
/// (and `Arc<dyn DataAdapter<E>>` on `Resource`) stay usable in the registry
/// without naming the concrete id type.
#[async_trait]
pub trait DataAdapter<E, Id = String>: Send + Sync
where
    E: Serialize + DeserializeOwned + Send + Sync + 'static,
    Id: Clone + Send + Sync + Serialize + DeserializeOwned + std::fmt::Display + std::str::FromStr,
{
    /// What this source can honestly do. Read once at boot and cached.
    fn capabilities(&self) -> Capabilities;

    /// One page of rows.
    async fn list(&self, query: &Query) -> Result<Page<E>, DataError>;

    /// One row by id; `Ok(None)` when absent.
    async fn get(&self, id: &Id) -> Result<Option<E>, DataError>;

    /// Many rows by id. Default: sequential `get`, skipping missing ids,
    /// preserving input order. Override for real batching (`IN (…)`,
    /// batch APIs). Returned order is not guaranteed.
    async fn get_many(&self, ids: &[Id]) -> Result<Vec<E>, DataError> {
        let mut out = Vec::with_capacity(ids.len());
        for id in ids {
            if let Some(e) = self.get(id).await? {
                out.push(e);
            }
        }
        Ok(out)
    }

    /// Create a record. Default: `Unsupported`.
    async fn create(
        &self,
        data: serde_json::Value,
        ctx: &WriteContext<'_>,
    ) -> Result<E, DataError> {
        let _ = (data, ctx);
        Err(DataError::Unsupported)
    }

    /// Merge a patch into a record. Default: `Unsupported`.
    async fn update(
        &self,
        id: &Id,
        patch: serde_json::Value,
        ctx: &WriteContext<'_>,
    ) -> Result<E, DataError> {
        let _ = (id, patch, ctx);
        Err(DataError::Unsupported)
    }

    /// Delete a record. Default: `Unsupported`.
    async fn delete(&self, id: &Id, ctx: &WriteContext<'_>) -> Result<(), DataError> {
        let _ = (id, ctx);
        Err(DataError::Unsupported)
    }

    /// Apply a batch of mutations. Default: sequential, stopping at the
    /// first error; `Upsert` creates, and on `Conflict` retries as update.
    async fn apply_mutations(
        &self,
        mutations: &[Mutation<Id>],
        ctx: &WriteContext<'_>,
    ) -> Result<(), DataError> {
        for m in mutations {
            match m {
                Mutation::Create { data } => {
                    self.create(data.clone(), ctx).await?;
                }
                Mutation::Update { id, patch } => {
                    self.update(id, patch.clone(), ctx).await?;
                }
                Mutation::Delete { id } => {
                    self.delete(id, ctx).await?;
                }
                Mutation::Upsert { id, data } => match self.create(data.clone(), ctx).await {
                    Ok(_) => {}
                    Err(DataError::Conflict) => {
                        self.update(id, data.clone(), ctx).await?;
                    }
                    Err(e) => return Err(e),
                },
            }
        }
        Ok(())
    }

    /// Begin a transaction. Default: `Unsupported` — the engine falls back
    /// to sequential mutations with per-row error reporting.
    async fn begin(&self) -> Result<Box<dyn TxAdapter<E, Id>>, DataError> {
        Err(DataError::Unsupported)
    }

    /// Aggregate over rows. Default: `Unsupported`.
    async fn aggregate(&self, agg: &Aggregation) -> Result<AggregationResult, DataError> {
        let _ = agg;
        Err(DataError::Unsupported)
    }

    /// Stream all matching rows. Default: page through `list` (fine up to
    /// tens of thousands of rows); SQL adapters override with `'static`
    /// keyset-cursor streams.
    ///
    /// Takes `Query` by value so the stream never borrows the query. The
    /// stream borrows the adapter (`'a`): a default paging through
    /// `list(&self)` cannot be `'static`, so adapters that need detached
    /// streams override with one over an `Arc`'d store. The default pages
    /// `Offset` requests 1, 2, … and follows `Page.next` for `Cursor`
    /// requests, ending when a page is empty or the adapter signals no
    /// `next`; a page failure yields one `Err` and ends the stream.
    async fn stream<'a>(&'a self, query: Query) -> BoxStream<'a, Result<E, DataError>> {
        use futures::stream::{self, StreamExt};

        let per_page = match query.pagination {
            Pagination::Offset { per_page, .. } | Pagination::Cursor { per_page, .. } => per_page,
        };
        let cursor_mode = matches!(query.pagination, Pagination::Cursor { .. });

        stream::unfold(
            (query.clone(), 1usize, None::<String>, false),
            move |(base, mut page_no, mut cursor, done)| {
                let this = self;
                async move {
                    if done {
                        return None;
                    }

                    let q = if cursor_mode {
                        let mut q = base.clone();
                        q.pagination = Pagination::Cursor {
                            after: cursor.take(),
                            before: None,
                            per_page,
                        };
                        q
                    } else {
                        let mut q = base.clone();
                        q.pagination = Pagination::Offset {
                            page: page_no,
                            per_page,
                        };
                        page_no += 1;
                        q
                    };

                    let page = match this.list(&q).await {
                        Ok(p) => p,
                        Err(e) => return Some((Err(e), (base, page_no, None, true))),
                    };

                    if page.items.is_empty() {
                        return None;
                    }
                    let next = page.next.map(|c| c.0);
                    let done = next.is_none();
                    Some((Ok(page.items), (base, page_no, next, done)))
                }
            },
        )
        .flat_map(|r| match r {
            Ok(items) => futures::future::Either::Left(stream::iter(items.into_iter().map(Ok))),
            Err(e) => futures::future::Either::Right(stream::once(async move { Err(e) })),
        })
        .boxed()
    }

    /// Introspect the source's schema. Default: `Unsupported`; sources with
    /// discoverable schemas return columns / mappings / a sample document.
    async fn describe(&self) -> Result<Vec<FieldSpec>, DataError> {
        Err(DataError::Unsupported)
    }

    /// Validate declared identifiers against the source. Default: `Ok(())`;
    /// the boot-time safety net for JSON/API adapters without compile-time
    /// checks.
    async fn validate(&self, identifiers: &[&str]) -> Result<(), DataError> {
        let _ = identifiers;
        Ok(())
    }
}

/// A transaction over one source.
///
/// Separate sub-trait so the main trait stays object-safe and read-only
/// adapters never see transaction machinery. `Id` mirrors `DataAdapter`.
#[async_trait]
pub trait TxAdapter<E, Id = String>: Send + Sync
where
    Id: Clone + Send + Sync + Serialize + DeserializeOwned + std::fmt::Display + std::str::FromStr,
{
    /// Read a row from the transaction's snapshot.
    async fn get(&self, id: &Id) -> Result<Option<E>, DataError>;

    /// Apply mutations to the transaction; all-or-nothing on commit.
    async fn apply(&mut self, mutations: &[Mutation<Id>]) -> Result<(), DataError>;

    /// Publish the transaction's changes.
    async fn commit(self: Box<Self>) -> Result<(), DataError>;

    /// Discard the transaction's changes.
    async fn rollback(self: Box<Self>) -> Result<(), DataError>;
}

pub use crate::error::DataError;
