use thiserror::Error;

pub type MspResult<T> = Result<T, MspError>;

#[derive(Debug, Error)]
pub enum MspError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("invalid hash digest: {0}")]
    InvalidHash(String),

    #[error("hash mismatch for {artifact}: expected {expected}, got {actual}")]
    HashMismatch {
        artifact: String,
        expected: String,
        actual: String,
    },

    #[error("schema validation failed for {schema}: {errors:?}")]
    SchemaValidation { schema: String, errors: Vec<String> },

    #[error("manifest validation failed: {0}")]
    ManifestValidation(String),

    #[error("skill not found: {0}")]
    SkillNotFound(String),

    #[error("pack not found: {0}")]
    PackNotFound(String),

    #[error("dependency not found: {0}")]
    DependencyNotFound(String),

    #[error("dependency cycle detected: {0}")]
    DependencyCycle(String),

    #[error("signature verification failed: {0}")]
    Signature(String),

    #[error("trust verification failed: {0}")]
    Trust(String),

    #[error("verification failed: {0}")]
    Verification(String),

    #[error("invalid request: {0}")]
    InvalidRequest(String),
}
