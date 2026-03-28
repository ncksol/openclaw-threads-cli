use thiserror::Error;

#[allow(dead_code)]
#[derive(Debug, Clone, Copy)]
pub enum ErrorCategory {
    Config,
    Validation,
    Auth,
    Network,
    Api,
    RateLimit,
    Database,
    AmbiguousPublish,
    Internal,
}

impl ErrorCategory {
    pub fn as_code(self) -> &'static str {
        match self {
            Self::Config => "CONFIG_ERROR",
            Self::Validation => "VALIDATION_ERROR",
            Self::Auth => "AUTH_ERROR",
            Self::Network => "NETWORK_ERROR",
            Self::Api => "API_ERROR",
            Self::RateLimit => "RATE_LIMIT_ERROR",
            Self::Database => "DATABASE_ERROR",
            Self::AmbiguousPublish => "AMBIGUOUS_PUBLISH_ERROR",
            Self::Internal => "INTERNAL_ERROR",
        }
    }

    #[allow(dead_code)]
    pub fn exit_code(self) -> i32 {
        match self {
            Self::Config => 2,
            Self::Validation => 3,
            Self::Auth => 4,
            Self::Network => 5,
            Self::Api => 6,
            Self::RateLimit => 7,
            Self::Database => 8,
            Self::AmbiguousPublish => 9,
            Self::Internal => 1,
        }
    }
}

#[derive(Debug, Error)]
#[error("{message}")]
pub struct CliError {
    pub category: ErrorCategory,
    pub message: String,
}

impl CliError {
    pub fn new(category: ErrorCategory, message: impl Into<String>) -> Self {
        Self {
            category,
            message: message.into(),
        }
    }
}

impl From<anyhow::Error> for CliError {
    fn from(value: anyhow::Error) -> Self {
        Self::new(ErrorCategory::Internal, value.to_string())
    }
}
