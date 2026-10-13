//! Port of `ui/src/utils/desktopUpdates.ts`: the desktop bridge must expose all four update methods.
pub const UPDATE_METHODS: [&str; 4] = ["startUpdate", "checkUpdates", "getUpdateStatus", "cancelUpdate"];

/// `desktopUpdates()`: `Err("updateUnavailable")` unless every update method is present on the bridge.
pub fn desktop_updates(bridge_methods: Option<&[&str]>) -> Result<(), &'static str> {
    match bridge_methods {
        Some(m) if UPDATE_METHODS.iter().all(|k| m.contains(k)) => Ok(()),
        _ => Err("updateUnavailable"),
    }
}
