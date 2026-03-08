mod bitmap_headers;
mod bmp;
mod error;
mod file_header;
mod types;

pub use bitmap_headers::{BitmapCoreHeader, BitmapHeader, BitmapInfoHeader, BitmapV4Header, BitmapV5Header};
pub use bmp::{BitmapCoreData, BitmapInfoData, BitmapV4Data, BitmapV5Data, Bmp, DibVariant};
pub use error::{
    BmpError, ColorMaskError, IccProfileError, IoStage, PixelDataLayoutError, StructuralError, ValidationError,
};
pub use file_header::FileHeader;
pub use types::{
    BitsPerPixel, CieXyz, CieXyzTriple, ColorMaskChannel, ColorMasks, Compression, FixedPoint2Dot30,
    FixedPoint16Dot16, GammaTriple, RgbMasks, RgbaMasks,
};

// Private helpers
mod helpers;
