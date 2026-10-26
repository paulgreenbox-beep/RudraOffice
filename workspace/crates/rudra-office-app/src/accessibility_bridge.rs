//! Transitional consumer facade. The shared adapter is owned by the launcher crate.
pub(crate) use rudra_office_launcher::accessibility::{
    AccessibilityUiAction, AccessibilityWindow, publish,
};
pub(crate) fn current_window() -> Result<AccessibilityWindow, String> {
    rudra_office_launcher::accessibility::current_window("Rudra Office")
}
