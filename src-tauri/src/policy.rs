use serde_json::Value;

pub fn validate_host(address: &str, port: u16, username: &str) -> Result<(), (&'static str, &'static str)> { if address.is_empty() || address.contains(['/', ':', ' ']) { return Err(("HOST_INVALID_ADDRESS", "地址只能是主机名或 IP，不能包含 URL、路径或 Shell 字符")); } if port == 0 { return Err(("VALIDATION", "端口必须在 1-65535 范围内")); } if username.is_empty() || username.contains([' ', '\\', '/']) { return Err(("VALIDATION", "用户名格式无效")); } Ok(()) }
pub fn is_fixed_readonly(argv: &[String]) -> bool { matches!(argv, [p, a] if (p == "df" && a == "-h")) || matches!(argv, [p] if p == "pwd" || p == "whoami") }
pub fn sanitize_text(input: &str) -> String { input.chars().filter(|c| !c.is_control() || *c == '\n' || *c == '\t').take(32_000).collect() }
pub fn validate_structured_command(value: &Value) -> bool { value.get("program").and_then(Value::as_str).is_some() && value.get("args").and_then(Value::as_array).is_some() }
