use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone)]
pub struct TokenSigner {
    secret: Vec<u8>,
    window: Duration,
}

impl TokenSigner {
    pub fn new(secret: Vec<u8>, window: Duration) -> Self {
        let window = if window.is_zero() {
            Duration::from_secs(1)
        } else {
            window
        };
        Self { secret, window }
    }

    pub fn issue(&self, username: &str, enc_key: [u8; 16]) -> u32 {
        let slot = current_slot(self.window);
        token_for_slot(&self.secret, username, enc_key, slot)
    }

    pub fn verify(&self, username: &str, enc_key: [u8; 16], token: u32) -> bool {
        let slot = current_slot(self.window);
        let current = token_for_slot(&self.secret, username, enc_key, slot);
        if token == current {
            return true;
        }

        if slot > 0 {
            let previous = token_for_slot(&self.secret, username, enc_key, slot - 1);
            if token == previous {
                return true;
            }
        }

        let next = token_for_slot(&self.secret, username, enc_key, slot + 1);
        token == next
    }
}

fn current_slot(window: Duration) -> u64 {
    let window_secs = window.as_secs().max(1);
    let now_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    now_secs / window_secs
}

fn token_for_slot(secret: &[u8], username: &str, enc_key: [u8; 16], slot: u64) -> u32 {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret).expect("HMAC can take key of any size");
    mac.update(username.as_bytes());
    mac.update(&enc_key);
    mac.update(&slot.to_le_bytes());
    let digest = mac.finalize().into_bytes();
    u32::from_le_bytes([digest[0], digest[1], digest[2], digest[3]])
}
