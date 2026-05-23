use serde::Serialize;
use std::collections::BTreeMap;
use tauri::command;

const SERVICE_NAME: &str = "meeting-minutes";
const SECRET_NAMES: [&str; 4] = [
    "groq_api_key",
    "gemini_api_key",
    "cloudflare_api_token",
    "deepgram_api_key",
];

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiSecretStatus {
    configured: BTreeMap<String, bool>,
}

pub fn is_allowed_secret_name(name: &str) -> bool {
    SECRET_NAMES.contains(&name)
}

fn validate_secret_name(name: &str) -> Result<&str, String> {
    if is_allowed_secret_name(name) {
        Ok(name)
    } else {
        Err(format!("api secret name is not allowed: {name}"))
    }
}

fn entry_for_secret(name: &str) -> Result<keyring::Entry, String> {
    let name = validate_secret_name(name)?;
    keyring::Entry::new(SERVICE_NAME, name)
        .map_err(|e| format!("failed to open OS keyring entry {name}: {e}"))
}

pub fn get_api_secret_value(name: &str) -> Result<Option<String>, String> {
    let entry = entry_for_secret(name)?;
    match entry.get_password() {
        Ok(value) => Ok(Some(value).filter(|value| !value.trim().is_empty())),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(format!("failed to read OS keyring secret {name}: {e}")),
    }
}

pub fn set_api_secret_value(name: &str, value: &str) -> Result<(), String> {
    let entry = entry_for_secret(name)?;
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(format!("failed to delete OS keyring secret {name}: {e}")),
        };
    }

    entry
        .set_password(trimmed)
        .map_err(|e| format!("failed to save OS keyring secret {name}: {e}"))
}

#[command]
pub fn get_api_secret(name: String) -> Result<Option<String>, String> {
    get_api_secret_value(&name)
}

#[command]
pub fn set_api_secret(name: String, value: String) -> Result<(), String> {
    set_api_secret_value(&name, &value)
}

#[command]
pub fn list_api_secret_status() -> Result<ApiSecretStatus, String> {
    let configured = SECRET_NAMES
        .iter()
        .map(|name| {
            get_api_secret_value(name)
                .map(|value| ((*name).to_string(), value.is_some()))
        })
        .collect::<Result<BTreeMap<_, _>, _>>()?;

    Ok(ApiSecretStatus { configured })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_known_api_secret_names_are_allowed() {
        assert!(is_allowed_secret_name("groq_api_key"));
        assert!(is_allowed_secret_name("gemini_api_key"));
        assert!(is_allowed_secret_name("cloudflare_api_token"));
        assert!(is_allowed_secret_name("deepgram_api_key"));
        assert!(!is_allowed_secret_name("cloudflare_account_id"));
        assert!(!is_allowed_secret_name("../groq_api_key"));
        assert!(!is_allowed_secret_name("random"));
    }

    #[test]
    fn validation_reports_unknown_secret_names_without_touching_keyring() {
        let err = validate_secret_name("not_allowed").unwrap_err();
        assert!(err.contains("not_allowed"));
    }
}
