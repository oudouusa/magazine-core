//! Wire protocol for the magazine-core plugin host.
//!
//! - [`framing`]: 4-byte BE length + canonical UTF-8 JSON frame codec.
//! - [`message`]: JSON-RPC 2.0 builders, [`message::Method`], version constants.
//! - [`golden`]: pinned conformance vectors (CONTRACT §9).
pub mod framing;
pub mod golden;
pub mod message;

pub use framing::{canonical_json, frame_bytes, read_frame, write_frame, FrameError, MAX_FRAME};
pub use message::{Method, PROTOCOL_VERSION, RECORD_SCHEMA_VERSION};
