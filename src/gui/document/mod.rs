//! Document/session state and derived inspection data for loaded BMP files.

mod inspect;
mod state;

pub(in crate::gui) use inspect::DocumentInspection;
pub(in crate::gui) use state::DocumentState;
