//! Runtime-agnostic building blocks shared by the native server ([`tabnest`])
//! and the Cloudflare Workers build ([`tabnest-worker`]).
//!
//! Everything in here must compile for `wasm32-unknown-unknown`: no filesystem,
//! no sockets, no clock. Callers pass timestamps and bytes in.

pub mod dto;
pub mod html;
pub mod image;
pub mod import;
pub mod net;
pub mod security;
pub mod session;
pub mod url;

pub use html::PageMeta;
