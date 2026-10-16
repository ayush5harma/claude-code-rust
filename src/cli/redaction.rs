// SPDX-License-Identifier: Apache-2.0

use serde_json::Value;

const REDACTED: &str = "[redacted]";
/// Name fragments that make a `name=value` or `name: value` pair secret. A fragment
/// matches inside a longer name (`CLAUDE_CODE_OAUTH_TOKEN`, `x-api-key`, `access_token`).
const SECRET_ASSIGNMENT_KEYS: &[&str] =
    &["api_key", "api-key", "apikey", "token", "secret", "password", "credential", "authorization"];
/// Credential schemes that precede the secret in an `Authorization` value.
const AUTH_SCHEMES: &[&str] = &["basic ", "digest ", "negotiate ", "bearer "];

pub fn redact_json_value(value: &mut Value) {
    match value {
        Value::Object(object) => {
            for (key, value) in object {
                if is_secret_key(key) {
                    *value = Value::String(REDACTED.to_owned());
                } else {
                    redact_json_value(value);
                }
            }
        }
        Value::Array(values) => {
            for value in values {
                redact_json_value(value);
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
}

pub fn redact_env_value(name: &str, value: Option<&str>) -> Option<String> {
    value.map(|value| if is_secret_key(name) { REDACTED.to_owned() } else { value.to_owned() })
}

/// Line-oriented redaction for text that reaches the always-on log, support bundles and
/// failure reports.
///
/// The TypeScript twin is `redactStartupDetail` in `agent-sdk/src/bridge/startup_failures.ts`.
/// This function is the reference: its table test (`redacts_credential_shapes`) lists the
/// shapes both must cover, and a rule added to one is added to the other.
pub fn redact_text(input: &str) -> String {
    input.lines().map(redact_line).collect::<Vec<_>>().join("\n")
}

pub fn redact_line(line: &str) -> String {
    let mut redacted = if let Ok(mut value) = serde_json::from_str::<Value>(line) {
        redact_json_value(&mut value);
        serde_json::to_string(&value).unwrap_or_else(|_| line.to_owned())
    } else {
        line.to_owned()
    };

    redacted = redact_bearer_tokens(&redacted);
    redacted = redact_sk_tokens(&redacted);
    redacted = redact_url_userinfo(&redacted);
    for key in SECRET_ASSIGNMENT_KEYS {
        redacted = redact_assignment_values(&redacted, key);
    }
    redacted
}

pub fn is_secret_key(key: &str) -> bool {
    let normalized = key.to_ascii_lowercase().replace('-', "_");
    normalized.contains("token")
        || normalized.contains("secret")
        || normalized.contains("api_key")
        || normalized.contains("apikey")
        || normalized.contains("authorization")
        || normalized.contains("password")
        || normalized.contains("credential")
        || normalized == "accesskey"
        || normalized == "refreshkey"
}

fn redact_bearer_tokens(input: &str) -> String {
    let mut output = input.to_owned();
    let mut search_start = 0;
    while let Some(relative_start) = find_ascii_case_insensitive(&output[search_start..], "Bearer ")
    {
        let start = search_start + relative_start;
        let value_start = start + "Bearer ".len();
        let value_end = output[value_start..]
            .find(is_value_delimiter)
            .map_or(output.len(), |offset| value_start + offset);
        if value_end <= value_start {
            break;
        }
        output.replace_range(value_start..value_end, REDACTED);
        search_start = value_start + REDACTED.len();
    }
    output
}

/// Provider keys (`sk-...`, `sk-ant-...`) wherever they appear, even without a name.
fn redact_sk_tokens(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut output = String::with_capacity(input.len());
    let mut copied = 0;
    let mut index = 0;
    while index + 3 < bytes.len() {
        let at_boundary = index == 0 || !is_word_byte(bytes[index - 1]);
        if at_boundary && bytes[index..index + 3].eq_ignore_ascii_case(b"sk-") {
            let end = index
                + 3
                + bytes[index + 3..].iter().take_while(|byte| is_token_byte(**byte)).count();
            if end > index + 3 {
                output.push_str(&input[copied..index]);
                output.push_str(REDACTED);
                copied = end;
                index = end;
                continue;
            }
        }
        index += 1;
    }
    output.push_str(&input[copied..]);
    output
}

/// `https://user:pass@host` keeps the scheme and host and drops the userinfo.
fn redact_url_userinfo(input: &str) -> String {
    let mut output = input.to_owned();
    for scheme in ["https://", "http://"] {
        let mut search_start = 0;
        while let Some(relative) = find_ascii_case_insensitive(&output[search_start..], scheme) {
            let start = search_start + relative + scheme.len();
            let userinfo_len = output.as_bytes()[start..]
                .iter()
                .take_while(|byte| !byte.is_ascii_whitespace() && **byte != b'/' && **byte != b'@')
                .count();
            if userinfo_len > 0 && output.as_bytes().get(start + userinfo_len) == Some(&b'@') {
                output.replace_range(start..start + userinfo_len, REDACTED);
                search_start = start + REDACTED.len();
            } else {
                search_start = start;
            }
        }
    }
    output
}

fn is_word_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

fn is_token_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-'
}

fn redact_assignment_values(input: &str, key: &str) -> String {
    let mut output = input.to_owned();
    let mut search_start = 0;

    while let Some(relative_key_start) = find_ascii_case_insensitive(&output[search_start..], key) {
        let key_start = search_start + relative_key_start;
        let name_end = key_start
            + key.len()
            + output.as_bytes()[key_start + key.len()..]
                .iter()
                .take_while(|byte| is_token_byte(**byte))
                .count();
        let Some((value_start, quote)) = assignment_value_start(&output, name_end) else {
            search_start = key_start + key.len();
            continue;
        };
        let scheme_len = if quote.is_none() { auth_scheme_len(&output[value_start..]) } else { 0 };
        let value_end = assignment_value_end(&output, value_start + scheme_len, quote);
        if value_end <= value_start {
            search_start = value_start;
            continue;
        }
        output.replace_range(value_start..value_end, REDACTED);
        search_start = value_start + REDACTED.len();
    }

    output
}

/// Length of a leading credential scheme plus its spaces (`Basic `), so the credential
/// after it is redacted with it instead of being left behind.
fn auth_scheme_len(value: &str) -> usize {
    let lowered = value.to_ascii_lowercase();
    AUTH_SCHEMES.iter().find(|scheme| lowered.starts_with(**scheme)).map_or(0, |scheme| {
        scheme.len() + value[scheme.len()..].bytes().take_while(|byte| *byte == b' ').count()
    })
}

fn assignment_value_start(input: &str, offset: usize) -> Option<(usize, Option<u8>)> {
    let bytes = input.as_bytes();
    let mut index = offset;
    if bytes.get(index).is_some_and(|byte| *byte == b'"' || *byte == b'\'') {
        index += 1;
    }
    while bytes.get(index).is_some_and(u8::is_ascii_whitespace) {
        index += 1;
    }
    if !bytes.get(index).is_some_and(|byte| *byte == b':' || *byte == b'=') {
        return None;
    }
    index += 1;
    while bytes.get(index).is_some_and(u8::is_ascii_whitespace) {
        index += 1;
    }
    let quote = bytes.get(index).copied().filter(|byte| *byte == b'"' || *byte == b'\'');
    if quote.is_some() {
        index += 1;
    }
    Some((index, quote))
}

fn assignment_value_end(input: &str, start: usize, quote: Option<u8>) -> usize {
    let bytes = input.as_bytes();
    let mut index = start;
    while let Some(byte) = bytes.get(index) {
        if quote.is_some_and(|quote| *byte == quote)
            || quote.is_none() && is_value_delimiter(char::from(*byte))
        {
            break;
        }
        index += 1;
    }
    index
}

fn find_ascii_case_insensitive(haystack: &str, needle: &str) -> Option<usize> {
    haystack.to_ascii_lowercase().find(&needle.to_ascii_lowercase())
}

fn is_value_delimiter(ch: char) -> bool {
    ch.is_ascii_whitespace() || matches!(ch, '"' | '\'' | ',' | ';' | '&' | '}')
}

#[cfg(test)]
mod tests {
    use super::{redact_env_value, redact_json_value, redact_line, redact_text};

    #[test]
    fn redacts_secret_json_keys_recursively() {
        let mut value = serde_json::json!({
            "model": "sonnet",
            "env": {
                "ANTHROPIC_API_KEY": "sk-ant-secret",
                "KEEP": "visible"
            },
            "claudeAiOauth": {
                "accessToken": "access",
                "refreshToken": "refresh"
            }
        });

        redact_json_value(&mut value);

        assert_eq!(value["model"], "sonnet");
        assert_eq!(value["env"]["KEEP"], "visible");
        assert_eq!(value["env"]["ANTHROPIC_API_KEY"], "[redacted]");
        assert_eq!(value["claudeAiOauth"]["accessToken"], "[redacted]");
        assert_eq!(value["claudeAiOauth"]["refreshToken"], "[redacted]");
    }

    #[test]
    fn redacts_secret_env_values_only() {
        assert_eq!(
            redact_env_value("ANTHROPIC_API_KEY", Some("sk-ant-secret")).as_deref(),
            Some("[redacted]")
        );
        assert_eq!(redact_env_value("PATH", Some("bin")).as_deref(), Some("bin"));
        assert_eq!(redact_env_value("PATH", None), None);
    }

    #[test]
    fn redacts_json_log_lines() {
        let line =
            r#"{"message":"x","fields":{"Authorization":"Bearer secret-token","keep":"ok"}}"#;

        let redacted = redact_line(line);

        assert!(redacted.contains(r#""keep":"ok""#));
        assert!(!redacted.contains("secret-token"));
        assert!(redacted.contains("[redacted]"));
    }

    #[test]
    fn redacts_common_text_credentials() {
        let text = "ANTHROPIC_API_KEY=sk-ant-secret\nAuthorization: Bearer oauth-secret\nraw Bearer raw-secret";

        let redacted = redact_text(text);

        assert!(!redacted.contains("sk-ant-secret"));
        assert!(!redacted.contains("oauth-secret"));
        assert!(!redacted.contains("raw-secret"));
        assert!(redacted.contains("ANTHROPIC_API_KEY=[redacted]"));
        assert!(redacted.contains("Authorization: [redacted]"));
        assert!(redacted.contains("Bearer [redacted]"));
    }

    // Mirrors the table in agent-sdk/src/bridge/startup_failures.test.ts.
    #[test]
    fn redacts_credential_shapes() {
        let cases: &[(&str, &str)] = &[
            ("GET /v1?token=abc&x=1", "GET /v1?token=[redacted]&x=1"),
            (
                "CLAUDE_CODE_OAUTH_TOKEN=sk-ant-oat01-abcDEF123",
                "CLAUDE_CODE_OAUTH_TOKEN=[redacted]",
            ),
            ("x-api-key: sk-ant-api03-secretvalue", "x-api-key: [redacted]"),
            ("access_token=abc123 next", "access_token=[redacted] next"),
            (
                "fetch https://user:pass@example.com/repo",
                "fetch https://[redacted]@example.com/repo",
            ),
            ("Authorization: Basic dXNlcjpwYXNz", "Authorization: [redacted]"),
            ("Authorization: Bearer abc.def", "Authorization: [redacted]"),
            ("key sk-live_abc123 end", "key [redacted] end"),
            ("task-1 and risk-free are not keys", "task-1 and risk-free are not keys"),
        ];
        for (input, expected) in cases {
            assert_eq!(redact_text(input), *expected, "input: {input}");
        }
    }
}
