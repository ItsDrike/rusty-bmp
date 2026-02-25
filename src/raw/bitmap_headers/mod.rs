mod any;
mod core;
mod info;
mod v4;
mod v5;

pub use any::BitmapHeader;
pub use core::BitmapCoreHeader;
pub use info::BitmapInfoHeader;
pub use v4::BitmapV4Header;
pub use v5::BitmapV5Header;
