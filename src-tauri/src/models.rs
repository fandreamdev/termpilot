use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Host {
    pub id: String,
    pub name: String,
    pub connection_type: String,
    pub address: String,
    pub port: u16,
    pub username: String,
    pub auth_method: String,
    pub group_name: Option<String>,
    pub is_production: bool,
    pub endpoint_fingerprint: Option<String>,
}
#[derive(Debug, Deserialize)]
pub struct HostUpsert {
    pub id: Option<String>,
    pub name: String,
    pub connection_type: String,
    pub address: String,
    pub port: u16,
    pub username: String,
    pub auth_method: String,
    pub group_name: Option<String>,
    pub is_production: bool,
    pub policy_id: String,
}
#[derive(Debug, Serialize)]
pub struct Session {
    pub id: String,
    pub host_id: String,
    pub status: String,
    pub started_at: String,
}
#[derive(Debug, Serialize)]
pub struct ErrorBody {
    pub code: String,
    pub message: String,
}
#[derive(Debug, Serialize)]
pub struct Envelope<T: Serialize> {
    pub ok: bool,
    pub request_id: String,
    pub data: Option<T>,
    pub error: Option<ErrorBody>,
}
impl<T: Serialize> Envelope<T> {
    pub fn ok(data: T) -> Self {
        Self {
            ok: true,
            request_id: uuid::Uuid::new_v4().to_string(),
            data: Some(data),
            error: None,
        }
    }
    pub fn err(code: &str, message: &str) -> Self {
        Self {
            ok: false,
            request_id: uuid::Uuid::new_v4().to_string(),
            data: None,
            error: Some(ErrorBody {
                code: code.into(),
                message: message.into(),
            }),
        }
    }
}
