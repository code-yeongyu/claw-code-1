use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use aws_config::default_provider::{
    credentials::DefaultCredentialsChain, region::DefaultRegionChain,
};
use aws_credential_types::{provider::ProvideCredentials, Credentials};
use aws_sigv4::{
    http_request::{SignableBody, SignableRequest, SigningSettings},
    sign::v4,
};
use aws_smithy_runtime_api::client::identity::Identity;
use serde::{Deserialize, Serialize};

use crate::error::ApiError;
use crate::types::{
    InputMessage, MessageDelta, MessageDeltaEvent, MessageRequest, MessageResponse,
    MessageStartEvent, MessageStopEvent, StreamEvent, ToolChoice, ToolDefinition,
};

use super::{preflight_message_request, Provider, ProviderFuture};

pub const DEFAULT_BASE_URL_TEMPLATE: &str = "https://bedrock-runtime.{region}.amazonaws.com";
const DEFAULT_ANTHROPIC_VERSION: &str = "bedrock-2023-05-31";
const DEFAULT_INITIAL_BACKOFF: Duration = Duration::from_millis(200);
const DEFAULT_MAX_BACKOFF: Duration = Duration::from_secs(2);
const DEFAULT_MAX_RETRIES: u32 = 2;
const DEFAULT_SERVICE_NAME: &str = "bedrock";
const REQUEST_ID_HEADER: &str = "x-amzn-requestid";
const ALT_REQUEST_ID_HEADER: &str = "x-amzn-request-id";

#[derive(Debug)]
enum CredentialsSource {
    Static(Credentials),
    Default,
}

#[derive(Debug)]
enum RegionSource {
    Static(String),
    Default,
}

#[derive(Debug, Clone)]
pub struct BedrockClient {
    http: reqwest::Client,
    credentials_source: Arc<CredentialsSource>,
    region_source: Arc<RegionSource>,
    base_url: Option<String>,
    max_retries: u32,
    initial_backoff: Duration,
    max_backoff: Duration,
}

impl BedrockClient {
    #[must_use]
    pub fn new(region: impl Into<String>, credentials: Credentials) -> Self {
        Self {
            http: reqwest::Client::new(),
            credentials_source: Arc::new(CredentialsSource::Static(credentials)),
            region_source: Arc::new(RegionSource::Static(region.into())),
            base_url: None,
            max_retries: DEFAULT_MAX_RETRIES,
            initial_backoff: DEFAULT_INITIAL_BACKOFF,
            max_backoff: DEFAULT_MAX_BACKOFF,
        }
    }

    pub fn from_env() -> Result<Self, ApiError> {
        Ok(Self {
            http: reqwest::Client::new(),
            credentials_source: Arc::new(CredentialsSource::Default),
            region_source: Arc::new(RegionSource::Default),
            base_url: read_base_url_override(),
            max_retries: DEFAULT_MAX_RETRIES,
            initial_backoff: DEFAULT_INITIAL_BACKOFF,
            max_backoff: DEFAULT_MAX_BACKOFF,
        })
    }

    #[must_use]
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = Some(base_url.into());
        self
    }

    #[must_use]
    pub fn with_retry_policy(
        mut self,
        max_retries: u32,
        initial_backoff: Duration,
        max_backoff: Duration,
    ) -> Self {
        self.max_retries = max_retries;
        self.initial_backoff = initial_backoff;
        self.max_backoff = max_backoff;
        self
    }

    pub async fn send_message(
        &self,
        request: &MessageRequest,
    ) -> Result<MessageResponse, ApiError> {
        let request = MessageRequest {
            stream: false,
            ..request.clone()
        };
        preflight_message_request(&request)?;
        let response = self.send_with_retry(&request).await?;
        let request_id = request_id_from_headers(response.headers());
        let mut payload = response.json::<MessageResponse>().await?;
        if payload.request_id.is_none() {
            payload.request_id = request_id;
        }
        Ok(payload)
    }

    pub async fn stream_message(
        &self,
        request: &MessageRequest,
    ) -> Result<MessageStream, ApiError> {
        let response = self.send_message(request).await?;
        Ok(MessageStream::from_response(response))
    }

    async fn send_with_retry(
        &self,
        request: &MessageRequest,
    ) -> Result<reqwest::Response, ApiError> {
        let mut attempts = 0;

        let last_error = loop {
            attempts += 1;
            let retryable_error = match self.send_raw_request(request).await {
                Ok(response) => match expect_success(response).await {
                    Ok(response) => return Ok(response),
                    Err(error) if error.is_retryable() && attempts <= self.max_retries + 1 => error,
                    Err(error) => return Err(error),
                },
                Err(error) if error.is_retryable() && attempts <= self.max_retries + 1 => error,
                Err(error) => return Err(error),
            };

            if attempts > self.max_retries {
                break retryable_error;
            }

            tokio::time::sleep(self.backoff_for_attempt(attempts)?).await;
        };

        Err(ApiError::RetriesExhausted {
            attempts,
            last_error: Box::new(last_error),
        })
    }

    async fn send_raw_request(
        &self,
        request: &MessageRequest,
    ) -> Result<reqwest::Response, ApiError> {
        let region = self.resolve_region().await?;
        let base_url = self
            .base_url
            .clone()
            .unwrap_or_else(|| default_base_url(&region));
        let request_url = invoke_model_endpoint(&base_url, &request.model)?;
        let request_body = serde_json::to_vec(&BedrockInvokeRequest::from(request))?;
        let signed_request = self
            .build_signed_request(&request_url, &region, &request_body)
            .await?;
        self.http
            .execute(signed_request)
            .await
            .map_err(ApiError::from)
    }

    async fn build_signed_request(
        &self,
        request_url: &str,
        region: &str,
        request_body: &[u8],
    ) -> Result<reqwest::Request, ApiError> {
        let host_header = host_header_value(request_url)?;
        let mut request = self
            .http
            .post(request_url)
            .header("accept", "application/json")
            .header("content-type", "application/json")
            .header("host", host_header)
            .body(request_body.to_vec())
            .build()?;
        let credentials = self.resolve_credentials().await?;
        let identity = Identity::new(credentials, None);

        let mut temp_request = http::Request::builder()
            .uri(request_url)
            .method(request.method().clone())
            .body(request_body.to_vec())
            .map_err(|error| {
                ApiError::Auth(format!("failed to build Bedrock signing request: {error}"))
            })?;
        temp_request.headers_mut().extend(request.headers().clone());

        let headers = temp_request
            .headers()
            .iter()
            .filter_map(|(name, value)| value.to_str().ok().map(|value| (name.as_str(), value)))
            .collect::<Vec<_>>();
        let signable_request = SignableRequest::new(
            request.method().as_str(),
            request_url,
            headers.into_iter(),
            SignableBody::Bytes(request_body),
        )
        .map_err(|error| {
            ApiError::Auth(format!(
                "failed to create Bedrock signable request: {error}"
            ))
        })?;
        let signing_params = v4::SigningParams::builder()
            .identity(&identity)
            .region(region)
            .name(DEFAULT_SERVICE_NAME)
            .time(SystemTime::now())
            .settings(SigningSettings::default())
            .build()
            .map_err(|error| {
                ApiError::Auth(format!("failed to build Bedrock signing params: {error}"))
            })?;
        let (signing_instructions, _) =
            aws_sigv4::http_request::sign(signable_request, &signing_params.into())
                .map_err(|error| {
                    ApiError::Auth(format!("failed to sign Bedrock request: {error}"))
                })?
                .into_parts();
        signing_instructions.apply_to_request_http1x(&mut temp_request);

        *request.headers_mut() = temp_request.headers().clone();
        Ok(request)
    }

    async fn resolve_region(&self) -> Result<String, ApiError> {
        match self.region_source.as_ref() {
            RegionSource::Static(region) => Ok(region.clone()),
            RegionSource::Default => DefaultRegionChain::builder()
                .build()
                .region()
                .await
                .map_or_else(
                || {
                    Err(ApiError::Auth(
                        "missing AWS region for Bedrock; export AWS_REGION or configure a profile region"
                            .to_string(),
                    ))
                },
                |region| Ok(region.to_string()),
            ),
        }
    }

    async fn resolve_credentials(&self) -> Result<Credentials, ApiError> {
        match self.credentials_source.as_ref() {
            CredentialsSource::Static(credentials) => Ok(credentials.clone()),
            CredentialsSource::Default => DefaultCredentialsChain::builder()
                .build()
                .await
                .provide_credentials()
                .await
                .map_err(|error| {
                    ApiError::Auth(format!(
                        "failed to load AWS credentials for Bedrock: {error}"
                    ))
                }),
        }
    }

    fn backoff_for_attempt(&self, attempt: u32) -> Result<Duration, ApiError> {
        let Some(multiplier) = 1_u32.checked_shl(attempt.saturating_sub(1)) else {
            return Err(ApiError::BackoffOverflow {
                attempt,
                base_delay: self.initial_backoff,
            });
        };
        Ok(self
            .initial_backoff
            .checked_mul(multiplier)
            .map_or(self.max_backoff, |delay| delay.min(self.max_backoff)))
    }
}

impl Provider for BedrockClient {
    type Stream = MessageStream;

    fn send_message<'a>(
        &'a self,
        request: &'a MessageRequest,
    ) -> ProviderFuture<'a, MessageResponse> {
        Box::pin(async move { self.send_message(request).await })
    }

    fn stream_message<'a>(
        &'a self,
        request: &'a MessageRequest,
    ) -> ProviderFuture<'a, Self::Stream> {
        Box::pin(async move { self.stream_message(request).await })
    }
}

#[derive(Debug)]
pub struct MessageStream {
    request_id: Option<String>,
    pending: VecDeque<StreamEvent>,
}

impl MessageStream {
    fn from_response(response: MessageResponse) -> Self {
        let request_id = response.request_id.clone();
        let mut pending = VecDeque::new();
        pending.push_back(StreamEvent::MessageStart(MessageStartEvent {
            message: response.clone(),
        }));
        pending.push_back(StreamEvent::MessageDelta(MessageDeltaEvent {
            delta: MessageDelta {
                stop_reason: response.stop_reason.clone(),
                stop_sequence: response.stop_sequence.clone(),
            },
            usage: response.usage.clone(),
        }));
        pending.push_back(StreamEvent::MessageStop(MessageStopEvent {}));
        Self {
            request_id,
            pending,
        }
    }

    #[must_use]
    pub fn request_id(&self) -> Option<&str> {
        self.request_id.as_deref()
    }

    pub async fn next_event(&mut self) -> Result<Option<StreamEvent>, ApiError> {
        Ok(self.pending.pop_front())
    }
}

#[derive(Debug, Serialize)]
struct BedrockInvokeRequest<'a> {
    anthropic_version: &'static str,
    max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<&'a String>,
    messages: &'a [InputMessage],
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<&'a Vec<ToolDefinition>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_choice: Option<&'a ToolChoice>,
}

impl<'a> From<&'a MessageRequest> for BedrockInvokeRequest<'a> {
    fn from(value: &'a MessageRequest) -> Self {
        Self {
            anthropic_version: DEFAULT_ANTHROPIC_VERSION,
            max_tokens: value.max_tokens,
            system: value.system.as_ref(),
            messages: &value.messages,
            tools: value.tools.as_ref(),
            tool_choice: value.tool_choice.as_ref(),
        }
    }
}

#[derive(Debug, Deserialize)]
struct BedrockErrorEnvelope {
    #[serde(default, rename = "__type")]
    aws_error_type: Option<String>,
    #[serde(default, rename = "type")]
    error_type: Option<String>,
    #[serde(default)]
    message: Option<String>,
}

async fn expect_success(response: reqwest::Response) -> Result<reqwest::Response, ApiError> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }

    let request_id = request_id_from_headers(response.headers());
    let body = response.text().await.unwrap_or_default();
    let parsed_error = serde_json::from_str::<BedrockErrorEnvelope>(&body).ok();

    Err(ApiError::Api {
        status,
        error_type: parsed_error
            .as_ref()
            .and_then(|error| error.aws_error_type.clone().or(error.error_type.clone())),
        message: parsed_error
            .as_ref()
            .and_then(|error| error.message.clone()),
        request_id,
        body,
        retryable: is_retryable_status(status),
    })
}

const fn is_retryable_status(status: reqwest::StatusCode) -> bool {
    matches!(status.as_u16(), 408 | 424 | 429 | 500 | 502 | 503 | 504)
}

fn read_base_url_override() -> Option<String> {
    std::env::var("BEDROCK_BASE_URL")
        .ok()
        .filter(|value| !value.is_empty())
}

fn default_base_url(region: &str) -> String {
    DEFAULT_BASE_URL_TEMPLATE.replace("{region}", region)
}

fn host_header_value(request_url: &str) -> Result<String, ApiError> {
    let parsed = reqwest::Url::parse(request_url)
        .map_err(|error| ApiError::Auth(format!("invalid Bedrock request url: {error}")))?;
    let host = parsed
        .host_str()
        .ok_or_else(|| ApiError::Auth("Bedrock request url is missing a host".to_string()))?;
    Ok(parsed
        .port()
        .map_or_else(|| host.to_string(), |port| format!("{host}:{port}")))
}

fn invoke_model_endpoint(base_url: &str, model_id: &str) -> Result<String, ApiError> {
    let trimmed = base_url.trim_end_matches('/');
    if trimmed.contains("/model/") && trimmed.ends_with("/invoke") {
        return Ok(trimmed.to_string());
    }

    let mut url = reqwest::Url::parse(&format!("{trimmed}/"))
        .map_err(|error| ApiError::Auth(format!("invalid Bedrock base url: {error}")))?;
    url.path_segments_mut()
        .map_err(|_| ApiError::Auth("invalid Bedrock base url path".to_string()))?
        .pop_if_empty()
        .push("model")
        .push(model_id)
        .push("invoke");
    Ok(url.to_string().trim_end_matches('/').to_string())
}

fn request_id_from_headers(headers: &reqwest::header::HeaderMap) -> Option<String> {
    headers
        .get(REQUEST_ID_HEADER)
        .or_else(|| headers.get(ALT_REQUEST_ID_HEADER))
        .and_then(|value| value.to_str().ok())
        .map(ToOwned::to_owned)
}

#[cfg(test)]
mod tests {
    use super::{
        default_base_url, invoke_model_endpoint, request_id_from_headers, BedrockClient,
        MessageStream,
    };
    use aws_credential_types::Credentials;
    use serde_json::json;

    use crate::types::{
        InputContentBlock, InputMessage, MessageRequest, MessageResponse, OutputContentBlock,
        StreamEvent, ToolChoice, ToolDefinition, Usage,
    };

    #[test]
    fn default_base_url_uses_region_template() {
        assert_eq!(
            default_base_url("us-east-1"),
            "https://bedrock-runtime.us-east-1.amazonaws.com"
        );
    }

    #[test]
    fn invoke_model_endpoint_appends_model_path_when_given_base_url() {
        assert_eq!(
            invoke_model_endpoint(
                "https://bedrock-runtime.us-east-1.amazonaws.com",
                "us.anthropic.claude-sonnet-4-6"
            )
            .expect("endpoint should build"),
            "https://bedrock-runtime.us-east-1.amazonaws.com/model/us.anthropic.claude-sonnet-4-6/invoke"
        );
    }

    #[test]
    fn invoke_model_endpoint_preserves_full_endpoint_override() {
        assert_eq!(
            invoke_model_endpoint(
                "https://example.test/model/us.anthropic.claude-sonnet-4-6/invoke",
                "ignored-model-id"
            )
            .expect("endpoint should preserve full override"),
            "https://example.test/model/us.anthropic.claude-sonnet-4-6/invoke"
        );
    }

    #[test]
    fn request_id_uses_bedrock_header_names() {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert("x-amzn-requestid", "req_primary".parse().expect("header"));
        assert_eq!(
            request_id_from_headers(&headers).as_deref(),
            Some("req_primary")
        );

        headers.clear();
        headers.insert("x-amzn-request-id", "req_alt".parse().expect("header"));
        assert_eq!(
            request_id_from_headers(&headers).as_deref(),
            Some("req_alt")
        );
    }

    #[test]
    fn synthetic_message_stream_replays_completed_response() {
        let response = MessageResponse {
            id: "msg_bedrock".to_string(),
            kind: "message".to_string(),
            role: "assistant".to_string(),
            content: vec![OutputContentBlock::Text {
                text: "Hello from Bedrock".to_string(),
            }],
            model: "us.anthropic.claude-sonnet-4-6".to_string(),
            stop_reason: Some("end_turn".to_string()),
            stop_sequence: None,
            usage: Usage {
                input_tokens: 11,
                cache_creation_input_tokens: 0,
                cache_read_input_tokens: 0,
                output_tokens: 5,
            },
            request_id: Some("req_bedrock_stream".to_string()),
        };

        let mut stream = MessageStream::from_response(response);
        let runtime = tokio::runtime::Runtime::new().expect("runtime");
        let events = runtime.block_on(async {
            let mut events = Vec::new();
            while let Some(event) = stream.next_event().await.expect("event") {
                events.push(event);
            }
            events
        });

        assert_eq!(stream.request_id(), Some("req_bedrock_stream"));
        assert!(matches!(events[0], StreamEvent::MessageStart(_)));
        assert!(matches!(events[1], StreamEvent::MessageDelta(_)));
        assert!(matches!(events[2], StreamEvent::MessageStop(_)));
    }

    #[test]
    fn bedrock_request_shape_includes_anthropic_version_and_tools() {
        let request = MessageRequest {
            model: "us.anthropic.claude-sonnet-4-6".to_string(),
            max_tokens: 64,
            messages: vec![InputMessage {
                role: "user".to_string(),
                content: vec![InputContentBlock::Text {
                    text: "hello bedrock".to_string(),
                }],
            }],
            system: Some("be helpful".to_string()),
            tools: Some(vec![ToolDefinition {
                name: "weather".to_string(),
                description: Some("Fetch weather".to_string()),
                input_schema: json!({"type": "object"}),
            }]),
            tool_choice: Some(ToolChoice::Auto),
            stream: true,
        };

        let payload = serde_json::to_value(super::BedrockInvokeRequest::from(&request))
            .expect("request should serialize");

        assert_eq!(payload["anthropic_version"], json!("bedrock-2023-05-31"));
        assert_eq!(payload["messages"][0]["role"], json!("user"));
        assert_eq!(payload["tools"][0]["name"], json!("weather"));
        assert_eq!(payload["tool_choice"], json!({"type": "auto"}));
        assert!(payload.get("stream").is_none());
    }

    #[test]
    fn explicit_constructor_keeps_static_credentials_and_region() {
        let client = BedrockClient::new(
            "us-east-1",
            Credentials::new("AKID", "SECRET", None, None, "test"),
        )
        .with_base_url("https://example.test");

        let debug = format!("{client:?}");
        assert!(debug.contains("example.test"));
    }
}
