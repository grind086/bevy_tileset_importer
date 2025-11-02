mod data;
mod image;

/// A macro that expands to a tileset extension string literal.
/// 
/// ```
/// const BASE: &str = tileset_ext_literal!();
/// const RON: &str = tileset_ext_literal!("ron");
/// 
/// assert_eq!(BASE, "ts");
/// assert_eq!(RON, "ts.ron");
/// ```
#[macro_export]
macro_rules! tileset_ext_literal {
    () => {
        "ts"
    };
    ($suf:literal) => {
        concat!($crate::tileset_ext_literal!(), ".", $suf)
    };
}

pub use data::*;
pub use image::*;

pub const TILESET_EXT: &str = tileset_ext_literal!();
