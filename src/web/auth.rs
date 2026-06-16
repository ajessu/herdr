use std::collections::HashMap;
use std::fmt::Write;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

fn bytes_to_hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(s, "{b:02x}");
    }
    s
}

pub fn generate_token() -> String {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).expect("failed to generate random bytes");
    bytes_to_hex(&bytes)
}

fn hash_token(token: &str) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    hasher.finalize().into()
}

pub(crate) struct WebAuthState {
    token_hash: [u8; 32],
    sessions: Mutex<HashMap<String, SessionEntry>>,
    session_ttl: Duration,
}

struct SessionEntry {
    expires_at: Instant,
}

impl WebAuthState {
    pub(crate) fn new(token: &str, session_ttl: Duration) -> Self {
        Self {
            token_hash: hash_token(token),
            sessions: Mutex::new(HashMap::new()),
            session_ttl,
        }
    }

    pub(crate) fn validate_token(&self, provided: &str) -> bool {
        let provided_hash = hash_token(provided);
        provided_hash.ct_eq(&self.token_hash).into()
    }

    pub(crate) fn create_session(&self) -> String {
        let mut bytes = [0u8; 32];
        getrandom::fill(&mut bytes).expect("failed to generate session id");
        let session_id = bytes_to_hex(&bytes);

        let entry = SessionEntry {
            expires_at: Instant::now() + self.session_ttl,
        };

        let mut sessions = self.sessions.lock().unwrap_or_else(|e| e.into_inner());
        self.prune_expired(&mut sessions);
        sessions.insert(session_id.clone(), entry);
        session_id
    }

    pub(crate) fn validate_session(&self, session_id: &str) -> bool {
        let mut sessions = self.sessions.lock().unwrap_or_else(|e| e.into_inner());
        self.prune_expired(&mut sessions);
        sessions
            .get(session_id)
            .is_some_and(|entry| entry.expires_at > Instant::now())
    }

    fn prune_expired(&self, sessions: &mut HashMap<String, SessionEntry>) {
        let now = Instant::now();
        sessions.retain(|_, entry| entry.expires_at > now);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_token_produces_64_hex_chars() {
        let token = generate_token();
        assert_eq!(token.len(), 64);
        assert!(token.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn generate_token_is_unique() {
        let t1 = generate_token();
        let t2 = generate_token();
        assert_ne!(t1, t2);
    }

    #[test]
    fn validate_correct_token() {
        let token = "my_secret_token";
        let state = WebAuthState::new(token, Duration::from_secs(3600));
        assert!(state.validate_token("my_secret_token"));
    }

    #[test]
    fn validate_wrong_token() {
        let token = "my_secret_token";
        let state = WebAuthState::new(token, Duration::from_secs(3600));
        assert!(!state.validate_token("wrong_token"));
    }

    #[test]
    fn validate_empty_token() {
        let token = "my_secret_token";
        let state = WebAuthState::new(token, Duration::from_secs(3600));
        assert!(!state.validate_token(""));
    }

    #[test]
    fn session_lifecycle() {
        let state = WebAuthState::new("token", Duration::from_secs(3600));
        let session_id = state.create_session();
        assert!(state.validate_session(&session_id));
        assert!(!state.validate_session("bogus_session"));
    }

    #[test]
    fn expired_session_rejected() {
        let state = WebAuthState::new("token", Duration::from_millis(0));
        let session_id = state.create_session();
        std::thread::sleep(Duration::from_millis(1));
        assert!(!state.validate_session(&session_id));
    }
}
