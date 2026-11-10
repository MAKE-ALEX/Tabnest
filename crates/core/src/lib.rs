//! 原生服务端（[`tabnest`]）与 Cloudflare Workers 构建（[`tabnest-worker`]）共用的
//! 运行时无关基础模块。
//!
//! 这里的代码必须能在 `wasm32-unknown-unknown` 目标上编译：不能直接使用文件系统、
//! socket 或系统时钟；需要时间戳或字节数据时，一律由调用方传入。

pub mod dto;
pub mod html;
pub mod image;
pub mod import;
pub mod net;
pub mod security;
pub mod session;
pub mod settings;
pub mod url;

// 页面元信息（网页标题、图标地址）类型，重新导出方便调用方直接引用。
pub use html::PageMeta;
