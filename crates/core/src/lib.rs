//! Core of the rustnior image viewer: decoding, caching, folder scanning and
//! view math. This crate has no UI dependencies.

pub mod cache;
pub mod cancel;
pub mod collection;
pub mod decode;
pub mod error;
pub mod exif;
pub mod loader;
pub mod natsort;
pub mod pixels;
pub mod transform;
pub mod viewport;

pub use cancel::CancelToken;
pub use collection::{Collection, Entry, ScanOptions, SortKey, SortOrder};
pub use decode::{DecodeRequest, Decoder, Decoders};
pub use error::{DecodeError, Result};
pub use loader::{LoadEvent, Loader, LoaderConfig};
pub use pixels::{DecodedImage, Frame, PixelFormat};
pub use transform::Orientation;
pub use viewport::Viewport;
