use thiserror::Error;

#[derive(Debug, Error)]
pub enum EngineError {
    #[error("JSON parse error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("dependency cycle detected involving element '{0}'")]
    CycleDetected(String),

    #[error("element not found: '{0}'")]
    ElementNotFound(String),

    #[error("expression evaluation error: {0}")]
    Eval(String),

    #[error("distribution sampling error: {0}")]
    Sampling(String),

    #[error("unsupported element type '{0}' (not yet implemented)")]
    Unsupported(String),

    #[error("lookup out of range for element '{0}': x={1}")]
    LookupRange(String, f64),

    #[error("invalid model: {0}")]
    InvalidModel(String),
}
