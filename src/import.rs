//! 书签导入的解析逻辑放在 [`tabnest_core::import`] 里，方便 Workers 构建复用；
//! 这个模块只是保留历史导入路径，把核心实现原样再导出一次。

pub use tabnest_core::import::*;
