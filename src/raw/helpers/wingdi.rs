//! In many cases, the Windows spec outlining the BMP format references
//! constants / "magic" values by their names in the wingdi.h header file,
//! sometimes without even mentioning what the underlying value actually is.
//!
//! This module contains the constant definitions that should match those in the
//! official wingdi.h Windows header file.
//!
//! These values were taken from the open-sourced Wine repository (a project
//! that allows running Windows programs from Linux), which contains this file.
//!
//! Link:
//! <https://gitlab.winehq.org/wine/wine/-/blob/master/include/wingdi.h>
//! Or a github mirror:
//! <https://github.com/wine-mirror/wine/blob/master/include/wingdi.h>
#![allow(non_upper_case_globals)] // we want to match the windows constant naming
#![allow(dead_code)] // we keep a broader set of spec constants than currently used

pub const BI_RGB: u32 = 0;
pub const BI_RLE8: u32 = 1;
pub const BI_RLE4: u32 = 2;
pub const BI_BITFIELDS: u32 = 3;
pub const BI_JPEG: u32 = 4;
pub const BI_PNG: u32 = 5;

pub const LCS_sRGB: u32 = 0x7352_4742; /* 'sRGB' */
pub const LCS_WINDOWS_COLOR_SPACE: u32 = 0x5769_6e20; /* 'Win ' */

pub const LCS_CALIBRATED_RGB: u32 = 0x0000_0000;
pub const LCS_DEVICE_RGB: u32 = 0x0000_0001;
pub const LCS_DEVICE_CMYK: u32 = 0x0000_0002;

pub const PROFILE_LINKED: u32 = 0x4c49_4e4b; /* 'LINK' */
pub const PROFILE_EMBEDDED: u32 = 0x4d42_4544; /* 'MBED' */

// The wine wingdi.h did have some, but not all of these defs, so these are
// taken from:
// https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-wmf/9fec0834-607d-427d-abd5-ab240fb0db38
pub const LCS_GM_ABS_COLORIMETRIC: u32 = 0x0000_0008;
pub const LCS_GM_BUSINESS: u32 = 0x0000_0001;
pub const LCS_GM_GRAPHICS: u32 = 0x0000_0002;
pub const LCS_GM_IMAGES: u32 = 0x0000_0004;
