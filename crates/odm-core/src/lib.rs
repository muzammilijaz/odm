//! Task queue, category rules, and download scheduler for ODM — Phase 2.
//! Sits on top of `odm-engine` (Phase 1/1b): persists the queue to SQLite,
//! resolves categories/destinations by file extension, and bounds concurrent
//! downloads.

mod categories;
mod db;
mod error;
mod manager;
mod model;
mod queue;
mod settings;

pub use db::Db;
pub use error::{CoreError, Result};
pub use manager::TaskManager;
pub use model::{Category, Task, TaskStatus};
pub use settings::{COOKIES_BROWSER, COOKIES_FILE};

pub use odm_engine::DownloadConfig;
