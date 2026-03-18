//! Save workflow state, validation, and background task handling.

mod quality;
mod state;
mod workflow;

pub(in crate::gui) use state::{SavePoll, SaveState};
