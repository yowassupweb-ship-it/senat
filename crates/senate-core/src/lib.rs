//! Senate core: the persistent state of a multi-agent environment.
//!
//! Three lines of continuity (see Senate.md §3): the specialist, the project, the obligation.
//! Everything here is model-agnostic and synchronous; model execution lives in `senate-runtime`.

pub mod cron;
pub mod engine;
pub mod error;
pub mod model;
pub mod tools;
pub mod schema;
pub mod store;
pub mod workset;

pub use error::{Error, Result};
pub use store::Store;

/// Unix time in milliseconds.
pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Time-sortable unique id.
pub fn new_id() -> String {
    uuid::Uuid::now_v7().simple().to_string()
}
