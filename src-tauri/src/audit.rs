use serde_json::Value;
use sha2::{Digest, Sha256};

pub const GENESIS: &str = "0000000000000000000000000000000000000000000000000000000000000000";
pub fn chain_hash(event_without_hash: &Value, prev_hash: &str) -> String {
    let canonical = serde_json::to_vec(event_without_hash).expect("event serializable");
    let mut h = Sha256::new();
    h.update(canonical);
    h.update(prev_hash.as_bytes());
    hex::encode(h.finalize())
}
