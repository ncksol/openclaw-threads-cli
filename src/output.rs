use serde::Serialize;
use serde_json::Value;

use crate::error::CliError;

#[derive(Debug, Clone, Copy)]
pub enum OutputMode {
    Human,
    Json,
}

#[derive(Serialize)]
struct JsonOutput {
    ok: bool,
    command: String,
    data: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonErrorBody>,
}

#[derive(Serialize)]
struct JsonErrorBody {
    code: String,
    message: String,
}

pub fn print_success<T: Serialize>(
    mode: OutputMode,
    command: impl Into<String>,
    human_text: impl AsRef<str>,
    data: T,
) {
    let command = command.into();
    match mode {
        OutputMode::Human => println!("{}: {}", command, redact_text(human_text.as_ref())),
        OutputMode::Json => {
            let payload = JsonOutput {
                ok: true,
                command,
                data: redact_json_value(serde_json::to_value(data).unwrap_or(Value::Null)),
                error: None,
            };
            println!(
                "{}",
                serde_json::to_string_pretty(&payload).unwrap_or_else(|_| "{}".to_string())
            );
        }
    }
}

pub fn print_error_and_exit(mode: OutputMode, command: &str, err: CliError) -> anyhow::Result<()> {
    let redacted_message = redact_text(&err.message);
    match mode {
        OutputMode::Human => eprintln!("{} {}: {}", command, err.category.as_code(), redacted_message),
        OutputMode::Json => {
            let payload = JsonOutput {
                ok: false,
                command: command.to_string(),
                data: Value::Null,
                error: Some(JsonErrorBody {
                    code: err.category.as_code().to_string(),
                    message: redacted_message,
                }),
            };
            eprintln!(
                "{}",
                serde_json::to_string_pretty(&payload).unwrap_or_else(|_| "{}".to_string())
            );
        }
    }
    std::process::exit(err.category.exit_code());
}

fn redact_json_value(value: Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut out = serde_json::Map::with_capacity(map.len());
            for (k, v) in map {
                if is_secret_key(&k) {
                    out.insert(k, Value::String("[REDACTED]".to_string()));
                } else {
                    out.insert(k, redact_json_value(v));
                }
            }
            Value::Object(out)
        }
        Value::Array(items) => Value::Array(items.into_iter().map(redact_json_value).collect()),
        Value::String(s) => Value::String(redact_text(&s)),
        other => other,
    }
}

fn is_secret_key(key: &str) -> bool {
    let normalized = key.to_ascii_lowercase();
    matches!(
        normalized.as_str(),
        "access_token"
            | "refresh_token"
            | "app_secret"
            | "client_secret"
            | "authorization"
            | "auth_header"
            | "auth"
    )
}

pub fn redact_text(input: &str) -> String {
    let mut out = input.to_string();
    for key in [
        "access_token",
        "refresh_token",
        "client_secret",
        "app_secret",
        "authorization",
        "auth_header",
    ] {
        out = redact_key_value_pair(&out, key);
    }
    out = redact_bearer_tokens(&out);
    out
}

fn redact_key_value_pair(input: &str, key: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut remaining = input;
    while let Some(idx) = remaining.find(key) {
        out.push_str(&remaining[..idx]);
        let after_key = &remaining[idx + key.len()..];
        if let Some(eq_idx) = after_key.find('=') {
            let (prefix, rest) = after_key.split_at(eq_idx + 1);
            out.push_str(key);
            out.push_str(prefix);
            let value_len = if let Some(quote) = rest.chars().next().filter(|c| *c == '"' || *c == '\'') {
                rest[1..]
                    .find(quote)
                    .map(|idx| idx + 2)
                    .unwrap_or(rest.len())
            } else {
                rest.find(|c: char| c == '&' || c == ' ' || c == ',' || c == '"' || c == '\'')
                    .unwrap_or(rest.len())
            };
            out.push_str("[REDACTED]");
            remaining = &rest[value_len..];
        } else {
            out.push_str(key);
            remaining = after_key;
        }
    }
    out.push_str(remaining);
    out
}

fn redact_bearer_tokens(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut remaining = input;
    let needle = "Bearer ";
    while let Some(idx) = remaining.find(needle) {
        out.push_str(&remaining[..idx]);
        out.push_str(needle);
        out.push_str("[REDACTED]");
        let after = &remaining[idx + needle.len()..];
        let value_len = after
            .find(|c: char| c == ' ' || c == ',' || c == '"' || c == '\'')
            .unwrap_or(after.len());
        remaining = &after[value_len..];
    }
    out.push_str(remaining);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_tokens_in_text() {
        let input = "access_token=abc refresh_token=def Authorization: Bearer xyz";
        let out = redact_text(input);
        assert!(!out.contains("abc"));
        assert!(!out.contains("def"));
        assert!(!out.contains("xyz"));
        assert!(out.contains("[REDACTED]"));
    }

    #[test]
    fn redacts_secret_fields_in_json_values() {
        let value = serde_json::json!({
            "access_token": "a",
            "nested": { "refresh_token": "b" },
            "items": [{ "authorization": "Bearer c" }]
        });
        let redacted = redact_json_value(value);
        assert_eq!(redacted["access_token"], "[REDACTED]");
        assert_eq!(redacted["nested"]["refresh_token"], "[REDACTED]");
        assert_eq!(redacted["items"][0]["authorization"], "[REDACTED]");
    }

    #[test]
    fn json_output_contract_has_stable_fields() {
        let success = JsonOutput {
            ok: true,
            command: "doctor check".to_string(),
            data: serde_json::json!({"k":"v"}),
            error: None,
        };
        let success_value = serde_json::to_value(success).expect("serialize success");
        assert_eq!(success_value["ok"], true);
        assert_eq!(success_value["command"], "doctor check");
        assert_eq!(success_value["data"]["k"], "v");
        assert!(success_value.get("error").is_none());

        let failure = JsonOutput {
            ok: false,
            command: "doctor check".to_string(),
            data: Value::Null,
            error: Some(JsonErrorBody {
                code: "CONFIG_ERROR".to_string(),
                message: "failed".to_string(),
            }),
        };
        let failure_value = serde_json::to_value(failure).expect("serialize failure");
        assert_eq!(failure_value["ok"], false);
        assert_eq!(failure_value["command"], "doctor check");
        assert_eq!(failure_value["data"], Value::Null);
        assert_eq!(failure_value["error"]["code"], "CONFIG_ERROR");
    }

    #[test]
    fn redact_text_handles_bearer_and_quoted_values() {
        let input =
            "authorization='Bearer supersecret' access_token=\"tok123\" refresh_token=ref123";
        let out = redact_text(input);
        assert!(!out.contains("supersecret"));
        assert!(!out.contains("tok123"));
        assert!(!out.contains("ref123"));
        assert!(out.contains("[REDACTED]"));
    }

    #[test]
    fn redact_json_value_redacts_nested_secret_keys_case_insensitive() {
        let value = serde_json::json!({
            "Access_Token": "top-secret",
            "meta": {
                "Auth_Header": "Bearer should-hide",
                "safe": "visible"
            }
        });
        let redacted = redact_json_value(value);
        assert_eq!(redacted["Access_Token"], "[REDACTED]");
        assert_eq!(redacted["meta"]["Auth_Header"], "[REDACTED]");
        assert_eq!(redacted["meta"]["safe"], "visible");
    }
}
