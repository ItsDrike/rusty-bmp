//! Side-panel state for brightness and contrast controls.

/// State for side-panel brightness/contrast controls.
pub(in crate::gui) struct TonalAdjustState {
    /// Pending brightness delta configured from side panel controls.
    pub(in crate::gui) brightness_input: i16,
    /// Pending contrast delta configured from side panel controls.
    pub(in crate::gui) contrast_input: i16,
}
