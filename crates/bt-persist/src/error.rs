//! Error types for the write path. The read path deliberately does **not**
//! use `Result`/`Err` for its failure modes — docs/M2-persistence-schema-v1.md
//! §5.4 requires every read failure to fall back to defaults rather than
//! propagate, so [`crate::read_settings`]/[`crate::read_session`] always
//! return a value; see [`crate::ReadReport`] for how the failure is reported
//! instead.

use std::io;
use std::path::PathBuf;

/// Failure writing a settings/session file. §5.3: "写失败…显式告警,绝不假装
/// 成功" — this type is what the caller inspects to build that alert; this
/// crate never panics on a write failure (`CONVENTIONS.md` §四 "panic = 数据
/// 丢失").
#[derive(Debug, thiserror::Error)]
pub enum WriteError {
    #[error("failed to serialize {what} to JSON: {source}")]
    Serialize {
        what: &'static str,
        #[source]
        source: serde_json::Error,
    },
    #[error("failed to write {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}
