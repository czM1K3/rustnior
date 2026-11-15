//! Core of the rustnior image viewer: decoding, caching, folder scanning and
//! view math. This crate has no UI dependencies.

pub mod cancel;
pub mod decode;
pub mod error;
pub mod pixels;
pub mod viewport;

pub use cancel::CancelToken;
pub use decode::{DecodeRequest, Decoder, Decoders};
pub use error::{DecodeError, Result};
pub use pixels::{DecodedImage, Frame, PixelFormat};
pub use viewport::Viewport;
