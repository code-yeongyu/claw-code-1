use std::env::VarError;
use std::fmt::{Display, Formatter};
use std::time::Duration;

use runtime::FailureClass;

const GENERIC_FATAL_WRAPPER_MARKERS: &[&str] = &[
    "something went wrong while processing your request",
    "please try again, or use /new to start a fresh session",
];

const CONTEXT_WINDOW_ERROR_MARKERS: &[&str] = &[
    "maximum context length",
    "context window",
    "context length",
    "too many tokens",
    "prompt is too long",
    "input is too long",
    "request is too large",
];

const OPENAI_BETAS_ERROR_MARKERS: &[&str] = &["betas"];

#[derive(Debug)]
pub enum ApiError {
    MissingCredentials {
        provider: &'static str,
        env_vars: &'static [&'static str],
    },
    ContextWindowExceeded {
        model: String,
        estimated_input_tokens: u32,
        requested_output_tokens: u32,
        estimated_total_tokens: u32,
        context_window_tokens: u32,
    },
    ExpiredOAuthToken,
    Auth(String),
    InvalidApiKeyEnv(VarError),
    Http(reqwest::Error),
    Io(std::io::Error),
    Json(serde_json::Error),
    Api {
        status: reqwest::StatusCode,
        error_type: Option<String>,
        message: Option<String>,
        request_id: Option<String>,
        body: String,
        retryable: bool,
    },
    RetriesExhausted {
        attempts: u32,
        last_error: Box<ApiError>,
    },
    InvalidSseFrame(&'static str),
    BackoffOverflow {
        attempt: u32,
        base_delay: Duration,
    },
}

impl ApiError {
    #[must_use]
    pub const fn missing_credentials(
        provider: &'static str,
        env_vars: &'static [&'static str],
    ) -> Self {
        Self::MissingCredentials { provider, env_vars }
    }

    #[must_use]
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::Http(error) => error.is_connect() || error.is_timeout() || error.is_request(),
            Self::Api { retryable, .. } => *retryable,
            Self::RetriesExhausted { last_error, .. } => last_error.is_retryable(),
            Self::MissingCredentials { .. }
            | Self::ContextWindowExceeded { .. }
            | Self::ExpiredOAuthToken
            | Self::Auth(_)
            | Self::InvalidApiKeyEnv(_)
            | Self::Io(_)
            | Self::Json(_)
            | Self::InvalidSseFrame(_)
            | Self::BackoffOverflow { .. } => false,
        }
    }

    #[must_use]
    pub fn request_id(&self) -> Option<&str> {
        match self {
            Self::Api { request_id, .. } => request_id.as_deref(),
            Self::RetriesExhausted { last_error, .. } => last_error.request_id(),
            Self::MissingCredentials { .. }
            | Self::ContextWindowExceeded { .. }
            | Self::ExpiredOAuthToken
            | Self::Auth(_)
            | Self::InvalidApiKeyEnv(_)
            | Self::Http(_)
            | Self::Io(_)
            | Self::Json(_)
            | Self::InvalidSseFrame(_)
            | Self::BackoffOverflow { .. } => None,
        }
    }

    #[must_use]
    pub fn status(&self) -> Option<reqwest::StatusCode> {
        match self {
            Self::Api { status, .. } => Some(*status),
            Self::RetriesExhausted { last_error, .. } => last_error.status(),
            Self::MissingCredentials { .. }
            | Self::ContextWindowExceeded { .. }
            | Self::ExpiredOAuthToken
            | Self::Auth(_)
            | Self::InvalidApiKeyEnv(_)
            | Self::Http(_)
            | Self::Io(_)
            | Self::Json(_)
            | Self::InvalidSseFrame(_)
            | Self::BackoffOverflow { .. } => None,
        }
    }

    #[must_use]
    pub fn detail(&self) -> Option<&str> {
        match self {
            Self::Api { message, body, .. } => {
                let detail = message.as_deref().unwrap_or(body).trim();
                (!detail.is_empty()).then_some(detail)
            }
            Self::RetriesExhausted { last_error, .. } => last_error.detail(),
            Self::MissingCredentials { .. }
            | Self::ContextWindowExceeded { .. }
            | Self::ExpiredOAuthToken
            | Self::Auth(_)
            | Self::InvalidApiKeyEnv(_)
            | Self::Http(_)
            | Self::Io(_)
            | Self::Json(_)
            | Self::InvalidSseFrame(_)
            | Self::BackoffOverflow { .. } => None,
        }
    }

    #[must_use]
    pub fn remediation_hint(&self) -> Option<&'static str> {
        match self.status().map(|status| status.as_u16()) {
            Some(400) if self.detail().is_some_and(looks_like_openai_betas_error) => Some(
                "Check OPENAI_BASE_URL. This usually means claw is pointed at an OpenAI-compatible endpoint that rejects Anthropic-style betas fields. Unset OPENAI_BASE_URL to use Anthropic directly, or point it at a compatible proxy.",
            ),
            Some(401) => Some(
                "Check your API credentials. Verify the relevant env var is set with a valid key/token: ANTHROPIC_AUTH_TOKEN, ANTHROPIC_API_KEY, OPENAI_API_KEY, or XAI_API_KEY.",
            ),
            Some(403) => Some(
                "Check your provider account, billing, and model access. This usually means your plan or organization does not allow this request yet.",
            ),
            Some(429) => Some(
                "You are being rate limited. Wait and retry, reduce request volume, or raise your provider rate limits if available.",
            ),
            _ => None,
        }
    }

    #[must_use]
    pub fn safe_failure_class(&self) -> &'static str {
        match self {
            Self::RetriesExhausted { .. } if self.is_context_window_failure() => "context_window",
            Self::RetriesExhausted { .. } if self.is_generic_fatal_wrapper() => {
                "provider_retry_exhausted"
            }
            Self::RetriesExhausted { last_error, .. } => last_error.safe_failure_class(),
            Self::MissingCredentials { .. } | Self::ExpiredOAuthToken | Self::Auth(_) => {
                "provider_auth"
            }
            Self::Api { status, .. } if matches!(status.as_u16(), 401 | 403) => "provider_auth",
            Self::ContextWindowExceeded { .. } => "context_window",
            Self::Api { .. } if self.is_context_window_failure() => "context_window",
            Self::Api { status, .. } if status.as_u16() == 429 => "provider_rate_limit",
            Self::Api { .. } if self.is_generic_fatal_wrapper() => "provider_internal",
            Self::Api { .. } => "provider_error",
            Self::Http(_) | Self::InvalidSseFrame(_) | Self::BackoffOverflow { .. } => {
                "provider_transport"
            }
            Self::InvalidApiKeyEnv(_) | Self::Io(_) | Self::Json(_) => "runtime_io",
        }
    }

    #[must_use]
    pub fn to_failure_class(&self) -> FailureClass {
        match self {
            Self::RetriesExhausted { last_error, .. } => last_error.to_failure_class(),
            Self::MissingCredentials { .. } | Self::ExpiredOAuthToken | Self::Auth(_) => {
                FailureClass::GatewayRouting
            }
            Self::Api { status, .. } if matches!(status.as_u16(), 401 | 403 | 429) => {
                FailureClass::GatewayRouting
            }
            Self::ContextWindowExceeded { .. } => FailureClass::PromptDelivery,
            Self::Api { .. }
            | Self::Http(_)
            | Self::InvalidSseFrame(_)
            | Self::BackoffOverflow { .. }
            | Self::InvalidApiKeyEnv(_)
            | Self::Io(_)
            | Self::Json(_) => FailureClass::Infra,
        }
    }

    #[must_use]
    pub fn is_generic_fatal_wrapper(&self) -> bool {
        match self {
            Self::Api { message, body, .. } => {
                message
                    .as_deref()
                    .is_some_and(looks_like_generic_fatal_wrapper)
                    || looks_like_generic_fatal_wrapper(body)
            }
            Self::RetriesExhausted { last_error, .. } => last_error.is_generic_fatal_wrapper(),
            Self::MissingCredentials { .. }
            | Self::ContextWindowExceeded { .. }
            | Self::ExpiredOAuthToken
            | Self::Auth(_)
            | Self::InvalidApiKeyEnv(_)
            | Self::Http(_)
            | Self::Io(_)
            | Self::Json(_)
            | Self::InvalidSseFrame(_)
            | Self::BackoffOverflow { .. } => false,
        }
    }

    #[must_use]
    pub fn is_context_window_failure(&self) -> bool {
        match self {
            Self::ContextWindowExceeded { .. } => true,
            Self::Api {
                status,
                message,
                body,
                ..
            } => {
                matches!(status.as_u16(), 400 | 413 | 422)
                    && (message
                        .as_deref()
                        .is_some_and(looks_like_context_window_error)
                        || looks_like_context_window_error(body))
            }
            Self::RetriesExhausted { last_error, .. } => last_error.is_context_window_failure(),
            Self::MissingCredentials { .. }
            | Self::ExpiredOAuthToken
            | Self::Auth(_)
            | Self::InvalidApiKeyEnv(_)
            | Self::Http(_)
            | Self::Io(_)
            | Self::Json(_)
            | Self::InvalidSseFrame(_)
            | Self::BackoffOverflow { .. } => false,
        }
    }
}

impl Display for ApiError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingCredentials { provider, env_vars } => write!(
                f,
                "missing {provider} credentials; export {} before calling the {provider} API",
                env_vars.join(" or ")
            ),
            Self::ContextWindowExceeded {
                model,
                estimated_input_tokens,
                requested_output_tokens,
                estimated_total_tokens,
                context_window_tokens,
            } => write!(
                f,
                "context_window_blocked for {model}: estimated input {estimated_input_tokens} + requested output {requested_output_tokens} = {estimated_total_tokens} tokens exceeds the {context_window_tokens}-token context window; compact the session or reduce request size before retrying"
            ),
            Self::ExpiredOAuthToken => {
                write!(
                    f,
                    "saved OAuth token is expired and no refresh token is available"
                )
            }
            Self::Auth(message) => write!(f, "auth error: {message}"),
            Self::InvalidApiKeyEnv(error) => {
                write!(f, "failed to read credential environment variable: {error}")
            }
            Self::Http(error) => write!(f, "http error: {error}"),
            Self::Io(error) => write!(f, "io error: {error}"),
            Self::Json(error) => write!(f, "json error: {error}"),
            Self::Api {
                status,
                error_type,
                message,
                request_id,
                body,
                ..
            } => {
                if let (Some(error_type), Some(message)) = (error_type, message) {
                    write!(f, "api returned {status} ({error_type})")?;
                    if let Some(request_id) = request_id {
                        write!(f, " [trace {request_id}]")?;
                    }
                    write!(f, ": {message}")
                } else {
                    write!(f, "api returned {status}")?;
                    if let Some(request_id) = request_id {
                        write!(f, " [trace {request_id}]")?;
                    }
                    write!(f, ": {body}")
                }
            }
            Self::RetriesExhausted {
                attempts,
                last_error,
            } => write!(f, "api failed after {attempts} attempts: {last_error}"),
            Self::InvalidSseFrame(message) => write!(f, "invalid sse frame: {message}"),
            Self::BackoffOverflow {
                attempt,
                base_delay,
            } => write!(
                f,
                "retry backoff overflowed on attempt {attempt} with base delay {base_delay:?}"
            ),
        }
    }
}

impl std::error::Error for ApiError {}

impl From<reqwest::Error> for ApiError {
    fn from(value: reqwest::Error) -> Self {
        Self::Http(value)
    }
}

impl From<std::io::Error> for ApiError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<serde_json::Error> for ApiError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}

impl From<VarError> for ApiError {
    fn from(value: VarError) -> Self {
        Self::InvalidApiKeyEnv(value)
    }
}

fn looks_like_generic_fatal_wrapper(text: &str) -> bool {
    let lowered = text.to_ascii_lowercase();
    GENERIC_FATAL_WRAPPER_MARKERS
        .iter()
        .any(|marker| lowered.contains(marker))
}

fn looks_like_context_window_error(text: &str) -> bool {
    let lowered = text.to_ascii_lowercase();
    CONTEXT_WINDOW_ERROR_MARKERS
        .iter()
        .any(|marker| lowered.contains(marker))
}

fn looks_like_openai_betas_error(text: &str) -> bool {
    let lowered = text.to_ascii_lowercase();
    OPENAI_BETAS_ERROR_MARKERS
        .iter()
        .any(|marker| lowered.contains(marker))
}

#[cfg(test)]
mod tests {
    use std::env::VarError;

    use runtime::FailureClass;

    use super::ApiError;

    #[test]
    fn detects_generic_fatal_wrapper_and_classifies_it_as_provider_internal() {
        let error = ApiError::Api {
            status: reqwest::StatusCode::INTERNAL_SERVER_ERROR,
            error_type: Some("api_error".to_string()),
            message: Some(
                "Something went wrong while processing your request. Please try again, or use /new to start a fresh session."
                    .to_string(),
            ),
            request_id: Some("req_jobdori_123".to_string()),
            body: String::new(),
            retryable: true,
        };

        assert!(error.is_generic_fatal_wrapper());
        assert_eq!(error.safe_failure_class(), "provider_internal");
        assert_eq!(error.request_id(), Some("req_jobdori_123"));
        assert!(error.to_string().contains("[trace req_jobdori_123]"));
    }

    #[test]
    fn retries_exhausted_preserves_nested_request_id_and_failure_class() {
        let error = ApiError::RetriesExhausted {
            attempts: 3,
            last_error: Box::new(ApiError::Api {
                status: reqwest::StatusCode::BAD_GATEWAY,
                error_type: Some("api_error".to_string()),
                message: Some(
                    "Something went wrong while processing your request. Please try again, or use /new to start a fresh session."
                        .to_string(),
                ),
                request_id: Some("req_nested_456".to_string()),
                body: String::new(),
                retryable: true,
            }),
        };

        assert!(error.is_generic_fatal_wrapper());
        assert_eq!(error.safe_failure_class(), "provider_retry_exhausted");
        assert_eq!(error.request_id(), Some("req_nested_456"));
    }

    #[test]
    fn classifies_provider_context_window_errors() {
        let error = ApiError::Api {
            status: reqwest::StatusCode::BAD_REQUEST,
            error_type: Some("invalid_request_error".to_string()),
            message: Some(
                "This model's maximum context length is 200000 tokens, but your request used 230000 tokens."
                    .to_string(),
            ),
            request_id: Some("req_ctx_123".to_string()),
            body: String::new(),
            retryable: false,
        };

        assert!(error.is_context_window_failure());
        assert_eq!(error.safe_failure_class(), "context_window");
        assert_eq!(error.request_id(), Some("req_ctx_123"));
    }

    #[test]
    fn maps_api_errors_to_claw_failure_taxonomy() {
        let cases = [
            (
                ApiError::missing_credentials("anthropic", &["ANTHROPIC_API_KEY"]),
                FailureClass::GatewayRouting,
            ),
            (ApiError::ExpiredOAuthToken, FailureClass::GatewayRouting),
            (
                ApiError::Auth("invalid bearer".to_string()),
                FailureClass::GatewayRouting,
            ),
            (
                ApiError::Api {
                    status: reqwest::StatusCode::TOO_MANY_REQUESTS,
                    error_type: None,
                    message: None,
                    request_id: None,
                    body: "rate limited".to_string(),
                    retryable: true,
                },
                FailureClass::GatewayRouting,
            ),
            (
                ApiError::ContextWindowExceeded {
                    model: "claude-sonnet".to_string(),
                    estimated_input_tokens: 10,
                    requested_output_tokens: 20,
                    estimated_total_tokens: 30,
                    context_window_tokens: 25,
                },
                FailureClass::PromptDelivery,
            ),
            (
                ApiError::Api {
                    status: reqwest::StatusCode::INTERNAL_SERVER_ERROR,
                    error_type: Some("api_error".to_string()),
                    message: Some("server exploded".to_string()),
                    request_id: None,
                    body: String::new(),
                    retryable: false,
                },
                FailureClass::Infra,
            ),
            (
                ApiError::InvalidApiKeyEnv(VarError::NotPresent),
                FailureClass::Infra,
            ),
            (
                ApiError::RetriesExhausted {
                    attempts: 3,
                    last_error: Box::new(ApiError::Api {
                        status: reqwest::StatusCode::UNAUTHORIZED,
                        error_type: None,
                        message: None,
                        request_id: None,
                        body: "unauthorized".to_string(),
                        retryable: false,
                    }),
                },
                FailureClass::GatewayRouting,
            ),
        ];

        for (error, expected) in cases {
            assert_eq!(error.to_failure_class(), expected);
        }
    }

    #[test]
    fn given_openai_betas_error_when_remediation_hint_requested_then_mentions_openai_base_url() {
        let error = ApiError::Api {
            status: reqwest::StatusCode::BAD_REQUEST,
            error_type: Some("invalid_request_error".to_string()),
            message: Some("betas: Extra inputs are not permitted".to_string()),
            request_id: Some("req_beta_123".to_string()),
            body: String::new(),
            retryable: false,
        };

        assert_eq!(error.status(), Some(reqwest::StatusCode::BAD_REQUEST));
        assert_eq!(
            error.detail(),
            Some("betas: Extra inputs are not permitted")
        );
        assert!(error
            .remediation_hint()
            .is_some_and(|hint| hint.contains("OPENAI_BASE_URL")));
    }

    #[test]
    fn given_retry_wrapped_auth_error_when_remediation_hint_requested_then_it_uses_nested_status() {
        let error = ApiError::RetriesExhausted {
            attempts: 2,
            last_error: Box::new(ApiError::Api {
                status: reqwest::StatusCode::UNAUTHORIZED,
                error_type: Some("authentication_error".to_string()),
                message: Some("Invalid bearer token".to_string()),
                request_id: Some("req_auth_123".to_string()),
                body: String::new(),
                retryable: false,
            }),
        };

        assert_eq!(error.status(), Some(reqwest::StatusCode::UNAUTHORIZED));
        assert_eq!(error.detail(), Some("Invalid bearer token"));
        assert!(error
            .remediation_hint()
            .is_some_and(|hint| hint.contains("ANTHROPIC_AUTH_TOKEN")));
    }
}
