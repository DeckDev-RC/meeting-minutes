use crate::HttpClientState;
use serde::{Deserialize, Serialize};
use tauri::command;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ApiValidationStatus {
    Valid,
    Missing,
    Invalid,
    Error,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiValidationInput {
    groq: String,
    gemini: String,
    cloudflare_account_id: String,
    cloudflare_api_token: String,
    deepgram_api_key: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiValidationResult {
    provider: String,
    status: ApiValidationStatus,
    message: String,
}

pub fn classify_api_validation_response(status: u16) -> ApiValidationStatus {
    match status {
        200..=299 => ApiValidationStatus::Valid,
        401 | 403 => ApiValidationStatus::Invalid,
        _ => ApiValidationStatus::Error,
    }
}

fn result(provider: &str, status: ApiValidationStatus, message: &str) -> ApiValidationResult {
    ApiValidationResult {
        provider: provider.to_string(),
        status,
        message: message.to_string(),
    }
}

async fn validate_get(provider: &str, request: reqwest::RequestBuilder) -> ApiValidationResult {
    let response = request
        .timeout(std::time::Duration::from_secs(12))
        .send()
        .await;
    let Ok(response) = response else {
        return result(
            provider,
            ApiValidationStatus::Error,
            "Falha de rede ou timeout.",
        );
    };
    match classify_api_validation_response(response.status().as_u16()) {
        ApiValidationStatus::Valid => result(provider, ApiValidationStatus::Valid, "Chave valida."),
        ApiValidationStatus::Invalid => result(
            provider,
            ApiValidationStatus::Invalid,
            "Chave recusada pelo provedor.",
        ),
        ApiValidationStatus::Error => result(
            provider,
            ApiValidationStatus::Error,
            &format!("Provedor respondeu HTTP {}.", response.status().as_u16()),
        ),
        ApiValidationStatus::Missing => result(provider, ApiValidationStatus::Missing, "Ausente."),
    }
}

#[command]
pub async fn validate_api_keys(
    state: tauri::State<'_, HttpClientState>,
    input: ApiValidationInput,
) -> Result<Vec<ApiValidationResult>, String> {
    let client = &state.0;
    let mut results = Vec::new();

    if input.groq.trim().is_empty() {
        results.push(result(
            "groq",
            ApiValidationStatus::Missing,
            "Chave Groq ausente.",
        ));
    } else {
        results.push(
            validate_get(
                "groq",
                client
                    .get("https://api.groq.com/openai/v1/models")
                    .bearer_auth(input.groq.trim()),
            )
            .await,
        );
    }

    if input.deepgram_api_key.trim().is_empty() {
        results.push(result(
            "deepgram",
            ApiValidationStatus::Missing,
            "Chave Deepgram ausente.",
        ));
    } else {
        results.push(
            validate_get(
                "deepgram",
                client.get("https://api.deepgram.com/v1/projects").header(
                    "Authorization",
                    format!("Token {}", input.deepgram_api_key.trim()),
                ),
            )
            .await,
        );
    }

    if input.gemini.trim().is_empty() {
        results.push(result(
            "gemini",
            ApiValidationStatus::Missing,
            "Chave Gemini ausente.",
        ));
    } else {
        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models?key={}",
            input.gemini.trim()
        );
        results.push(validate_get("gemini", client.get(url)).await);
    }

    if input.cloudflare_account_id.trim().is_empty() || input.cloudflare_api_token.trim().is_empty()
    {
        results.push(result(
            "cloudflare",
            ApiValidationStatus::Missing,
            "Account ID ou token Cloudflare ausente.",
        ));
    } else {
        let url = format!(
            "https://api.cloudflare.com/client/v4/accounts/{}/ai/models/search?search=whisper",
            input.cloudflare_account_id.trim()
        );
        results.push(
            validate_get(
                "cloudflare",
                client
                    .get(url)
                    .bearer_auth(input.cloudflare_api_token.trim()),
            )
            .await,
        );
    }

    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_provider_validation_status_codes() {
        assert_eq!(
            classify_api_validation_response(200),
            ApiValidationStatus::Valid
        );
        assert_eq!(
            classify_api_validation_response(401),
            ApiValidationStatus::Invalid
        );
        assert_eq!(
            classify_api_validation_response(403),
            ApiValidationStatus::Invalid
        );
        assert_eq!(
            classify_api_validation_response(429),
            ApiValidationStatus::Error
        );
        assert_eq!(
            classify_api_validation_response(500),
            ApiValidationStatus::Error
        );
    }
}
