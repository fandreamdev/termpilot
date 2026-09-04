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
pub fn validate_structured_command(value: &Value) -> bool {
    value.get("program").and_then(Value::as_str).is_some()
        && value.get("args").and_then(Value::as_array).is_some()
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
    }
}
