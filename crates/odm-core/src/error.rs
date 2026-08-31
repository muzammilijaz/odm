#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),

    #[error("engine error: {0}")]
    Engine(#[from] odm_engine::EngineError),

    #[error("task {0} not found")]
    TaskNotFound(i64),

    #[error("task {0} has no in-memory handle (not currently running)")]
    TaskNotRunning(i64),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("background task join error")]
    Join,
}

pub type Result<T> = std::result::Result<T, CoreError>;
