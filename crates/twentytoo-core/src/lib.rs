#![warn(missing_docs)]

//! Twentytoo core: the trait and type surface every other slice builds on.
//!
//! This crate holds the Phase-1 contract — the query model, capabilities,
//! write context, field definitions, policy/action/resource traits, the v2
//! [`DataAdapter`] trait, and the [`InMemoryAdapter`] reference
//! implementation that proves the contract and powers the demo.

pub mod action;
pub mod actor;
pub mod adapter;
pub mod aggregation;
pub mod audit;
pub mod capabilities;
pub mod error;
pub mod field;
pub mod in_memory;
pub mod policy;
pub mod query;
pub mod resource;
pub mod write;

pub use crate::action::{Action, ActionError, ActionField, ActionResult, ActionScope};
pub use crate::actor::Actor;
pub use crate::adapter::{DataAdapter, DataError, TxAdapter};
pub use crate::aggregation::{Aggregation, AggregationResult, Bucket, GroupBy, Interval, Measure};
pub use crate::audit::{AuditAction, AuditEvent, EventResource};
pub use crate::capabilities::{
    AggregationCapability, Capabilities, ConcurrencySupport, PaginationModes, WriteCapability,
};
pub use crate::field::{Field, FieldKind, FieldSpec};
pub use crate::in_memory::InMemoryAdapter;
pub use crate::policy::{DenyAll, Policy};
pub use crate::query::{
    Cursor, FilterNode, FilterOp, FilterValue, NullsOrder, Page, Pagination, Query, SearchMode,
    SearchSpec, SortDir, SortField,
};
pub use crate::resource::{FilterSpec, Relationship, Resource};
pub use crate::write::{Mutation, Version, WriteContext};

/// One-stop import for the common consumer surface.
pub mod prelude {
    pub use crate::action::{Action, ActionError, ActionField, ActionResult, ActionScope};
    pub use crate::actor::Actor;
    pub use crate::adapter::{DataAdapter, TxAdapter};
    pub use crate::aggregation::{
        Aggregation, AggregationResult, Bucket, GroupBy, Interval, Measure,
    };
    pub use crate::audit::{AuditAction, AuditEvent, EventResource};
    pub use crate::capabilities::{
        AggregationCapability, Capabilities, ConcurrencySupport, PaginationModes, WriteCapability,
    };
    pub use crate::error::DataError;
    pub use crate::field::{Field, FieldKind, FieldSpec};
    pub use crate::in_memory::InMemoryAdapter;
    pub use crate::policy::{DenyAll, Policy};
    pub use crate::query::{
        Cursor, FilterNode, FilterOp, FilterValue, NullsOrder, Page, Pagination, Query, SearchMode,
        SearchSpec, SortDir, SortField,
    };
    pub use crate::resource::{FilterSpec, Relationship, Resource};
    pub use crate::write::{Mutation, Version, WriteContext};
    pub use crate::{field, fields};
}
