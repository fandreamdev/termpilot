use serde_json::Value;

pub fn validate_host(
    address: &str,
    port: u16,
    username: &str,
) -> Result<(), (&'static str, &'static str)> {
    if address.is_empty() || address.contains(['/', ':', ' ']) {
        return Err((
            "HOST_INVALID_ADDRESS",
            "地址只能是主机名或 IP，不能包含 URL、路径或 Shell 字符",
        ));
    }
    if port == 0 {
        return Err(("VALIDATION", "端口必须在 1-65535 范围内"));
    }
    if username.is_empty() || username.contains([' ', '\\', '/']) {
        return Err(("VALIDATION", "用户名格式无效"));
    }
    Ok(())
}
pub fn validate_fingerprint(value: &str) -> bool {
    let value = value.strip_prefix("SHA256:").unwrap_or(value);
    (16..=128).contains(&value.len())
        && value
            .chars()
            .all(|c| c.is_ascii_hexdigit() || c == ':' || c == '/')
}
pub fn is_fixed_readonly(argv: &[String]) -> bool {
    matches!(argv, [p, a] if (p == "df" && a == "-h"))
        || matches!(argv, [p] if p == "pwd" || p == "whoami")
}
pub fn sanitize_text(input: &str) -> String {
    input
        .chars()
        .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
        .take(32_000)
        .collect()
}
pub fn redact_sensitive(input: &str) -> String {
    sanitize_text(input)
        .lines()
        .map(|line| {
            let lower = line.to_ascii_lowercase();
            if [
                "password",
                "passwd",
                "token",
                "secret",
                "private_key",
                "api_key",
                "authorization",
                "cookie",
            ]
            .iter()
            .any(|needle| lower.contains(needle))
            {
                "[REDACTED]"
            } else {
                line
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}
pub fn validate_structured_command(value: &Value) -> bool {
    let Some(program) = value.get("program").and_then(Value::as_str) else {
        return false;
    };
    let Some(args) = value.get("args").and_then(Value::as_array) else {
        return false;
    };
    !program.is_empty()
        && program.len() <= 256
        && !is_forbidden_program(program)
        && args.len() <= 64
        && args.iter().all(|arg| {
            arg.as_str()
                .map(|s| !s.is_empty() && s.len() <= 4096 && !contains_shell_metacharacters(s))
                .unwrap_or(false)
        })
}

fn contains_shell_metacharacters(value: &str) -> bool {
    value
        .chars()
        .any(|c| matches!(c, '\0' | '\n' | '\r' | '|' | ';' | '&' | '>' | '<' | '`'))
}

pub fn is_forbidden_program(program: &str) -> bool {
    matches!(
        program,
        "sh" | "bash"
            | "zsh"
            | "fish"
            | "cmd"
            | "powershell"
            | "pwsh"
            | "ssh"
            | "sudo"
            | "eval"
            | "xargs"
    )
}

pub fn command_risk(argv: &[String]) -> &'static str {
    if argv.is_empty()
        || is_forbidden_program(&argv[0])
        || argv.iter().any(|arg| contains_shell_metacharacters(arg))
    {
        return "blocked";
    }
    if is_fixed_readonly(argv) {
        "low"
    } else {
        "medium"
    }
}

pub fn rule_matches(
    rule: &Value,
    argv: &[String],
    host_id: &str,
    remote_user: &str,
    cwd: &str,
) -> bool {
    let program_ok = rule
        .get("program")
        .and_then(Value::as_str)
        .map(|v| argv.first().map(String::as_str) == Some(v))
        .unwrap_or(false);
    let args_ok = rule
        .get("args")
        .and_then(Value::as_array)
        .map(|expected| {
            expected.len() == argv.len().saturating_sub(1)
                && expected
                    .iter()
                    .zip(argv.iter().skip(1))
                    .all(|(a, b)| a.as_str() == Some(b))
        })
        .unwrap_or(false);
    program_ok
        && args_ok
        && rule.get("host_id").and_then(Value::as_str) == Some(host_id)
        && rule.get("remote_user").and_then(Value::as_str) == Some(remote_user)
        && rule.get("cwd").and_then(Value::as_str) == Some(cwd)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readonly_templates_are_exact() {
        assert!(is_fixed_readonly(&["df".into(), "-h".into()]));
        assert!(is_fixed_readonly(&["pwd".into()]));
        assert!(!is_fixed_readonly(&["df".into(), "-H".into()]));
        assert!(!is_fixed_readonly(&[
            "sh".into(),
            "-c".into(),
            "pwd".into()
        ]));
    }

    #[test]
    fn sanitization_removes_control_chars_and_limits_size() {
        assert_eq!(sanitize_text("hello\u{0000}\nworld"), "hello\nworld");
        assert!(sanitize_text(&"x".repeat(40_000)).len() <= 32_000);
    }

    #[test]
    fn structured_commands_require_program_and_args() {
        assert!(validate_structured_command(
            &serde_json::json!({"program":"pwd","args":[]})
        ));
        assert!(!validate_structured_command(
            &serde_json::json!({"program":"pwd"})
        ));
        assert!(!validate_structured_command(
            &serde_json::json!({"program":"sh","args":["-c","pwd"]})
        ));
        assert_eq!(
            command_risk(&["rm".into(), "-rf".into(), "/".into()]),
            "medium"
        );
        assert_eq!(
            command_risk(&["bash".into(), "-c".into(), "pwd".into()]),
            "blocked"
        );
    }
}
