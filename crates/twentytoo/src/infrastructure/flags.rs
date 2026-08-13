//! Feature-flag registry.

use std::collections::HashMap;

use parking_lot::RwLock;

/// In-memory flag state.
///
/// A flag that was never `set` is **off** — the conservative reading (`05`
/// §6: "a non-existent flag → false"). `set` is the whole API this slice
/// needs; targeting strategies land with the flags slice.
#[derive(Default)]
pub struct FlagService {
    enabled: RwLock<HashMap<String, bool>>,
}

impl FlagService {
    /// An empty registry: every flag off.
    pub fn new() -> Self {
        return Self::default();
    }

    /// Turn `name` on or off.
    pub fn set(&self, name: &str, enabled: bool) {
        self.enabled.write().insert(name.to_string(), enabled);
    }

    /// Whether `name` is currently on.
    pub fn enabled(&self, name: &str) -> bool {
        return self.enabled.read().get(name).copied().unwrap_or(false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unset_flags_are_off() {
        let f = FlagService::new();
        assert!(!f.enabled("billing"));
    }

    #[test]
    fn set_toggles() {
        let f = FlagService::new();
        f.set("billing", true);
        assert!(f.enabled("billing"));
        f.set("billing", false);
        assert!(!f.enabled("billing"));
    }
}
