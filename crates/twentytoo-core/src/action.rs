//! Custom actions: buttons that run engine-side logic over records.

use async_trait::async_trait;

use crate::actor::Actor;
use crate::field::FieldKind;

/// Where an action may run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ActionScope {
    /// On one record (detail view button).
    Record,
    /// On a selection of records (list view bulk button).
    Bulk,
    /// Without any record (top-level button).
    Standalone,
}

/// One input field of an action's form.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActionField {
    /// Machine name.
    pub name: String,
    /// Human label.
    pub label: String,
    /// Rendering kind.
    pub kind: FieldKind,
    /// Required in the form.
    pub required: bool,
}

/// What an action reports back to the UI.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ActionResult {
    /// Done; show the message.
    Success {
        /// Message to display.
        message: String,
    },
    /// Done; navigate to the URL.
    Redirect {
        /// Target URL.
        url: String,
    },
}

/// An action failure
#[derive(Debug)]
pub enum ActionError {
    /// The actor lacks permission
    Forbidden,
    /// The input failed validation
    Validation(String),
    /// Any other failure
    Internal(Box<dyn std::error::Error + Send + Sync>),
}

impl std::fmt::Display for ActionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ActionError::Forbidden => return write!(f, "forbidden"),
            ActionError::Validation(msg) => return write!(f, "validation error: {msg}"),
            ActionError::Internal(e) => return write!(f, "internal error: {e}"),
        }
    }
}

impl std::error::Error for ActionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ActionError::Internal(e) => return Some(e.as_ref()),
            _ => return None,
        }
    }
}

/// A custom action over one or more records of a resource.
#[async_trait]
pub trait Action<E>: Send + Sync {
    /// Stable key, used in URLs and permission strings (`"archive"`).
    fn key(&self) -> &'static str;

    /// Button label.
    fn label(&self) -> &'static str;

    /// Where the action appears.
    fn scope(&self) -> ActionScope;

    /// Ask for confirmation before running.
    fn requires_confirmation(&self) -> bool {
        return false;
    }

    /// Form fields, for actions that take input.
    fn input_fields(&self) -> Vec<ActionField> {
        return Vec::new();
    }

    /// Permission required, `"resource.action"` form.
    fn policy(&self) -> &'static str;

    /// Feature flag gating this action.
    fn flag(&self) -> Option<&'static str> {
        return None;
    }

    /// Run the action. `record` is `None` for `Standalone` actions.
    #[allow(clippy::implicit_return)]
    async fn execute(
        &self,
        record: &mut E,
        actor: &Actor,
        input: serde_json::Value,
    ) -> Result<ActionResult, ActionError>;
}
