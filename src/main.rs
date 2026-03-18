#![allow(
    clippy::many_single_char_names,
    clippy::similar_names,
    clippy::items_after_statements,
    clippy::too_many_lines,
    clippy::cognitive_complexity,
    clippy::suboptimal_flops
)]

mod gui;

fn main() -> Result<(), eframe::Error> {
    gui::run()
}
