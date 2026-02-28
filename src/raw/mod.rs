mod bitmap_headers;
mod bmp;
mod error;
mod file_header;
mod types;

pub use bitmap_headers::{BitmapCoreHeader, BitmapHeader, BitmapInfoHeader, BitmapV4Header, BitmapV5Header};
pub use bmp::Bmp;
pub use error::{BmpError, BmpResult};
pub use file_header::FileHeader;
pub use types::{
    CieXyz, CieXyzTriple, ColorMaskChannel, ColorMasks, FixedPoint2Dot30, FixedPoint16Dot16, GammaTriple, RgbMasks,
    RgbaMasks, BitsPerPixel, Compression,
};

// Private helpers
mod helpers;
