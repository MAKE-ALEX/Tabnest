use sha2::{Digest, Sha256};
use worker::Env;

pub struct Config {
    pub password: Option<String>,
    /// HMAC key for the stateless session cookie.
    pub session_secret: Vec<u8>,
    /// Allow metadata / favicon fetching from private and loopback addresses.
    pub allow_private_network: bool,
    /// Add the `Secure` attribute to the session cookie.
    pub cookie_secure: bool,
    pub seed_demo: bool,
    pub version: &'static str,
}

fn var(env: &Env, name: &str) -> Option<String> {
    env.var(name)
        .ok()
        .map(|value| value.to_string())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn flag(env: &Env, name: &str) -> bool {
    var(env, name)
        .map(|value| {
            matches!(
                value.to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
}

impl Config {
    pub fn from_env(env: &Env) -> Self {
        let password = var(env, "APP_PASSWORD");

        // Without an explicit SESSION_SECRET we derive one from the password so
        // a fresh deployment still works. Changing the password then rotates
        // every session, which is the desired behaviour anyway.
        let session_secret = var(env, "SESSION_SECRET")
            .map(|value| value.into_bytes())
            .unwrap_or_else(|| {
                let seed = password
                    .clone()
                    .unwrap_or_else(|| "tabnest-insecure-default".to_string());
                let mut hasher = Sha256::new();
                hasher.update(seed.as_bytes());
                hasher.update(b"tabnest-worker-session-v1");
                hasher.finalize().to_vec()
            });

        Self {
            password,
            session_secret,
            allow_private_network: flag(env, "ALLOW_PRIVATE_NETWORK"),
            cookie_secure: var(env, "COOKIE_SECURE")
                .map(|value| {
                    !matches!(
                        value.to_ascii_lowercase().as_str(),
                        "0" | "false" | "no" | "off"
                    )
                })
                .unwrap_or(true),
            seed_demo: var(env, "SEED_DEMO")
                .map(|value| {
                    !matches!(
                        value.to_ascii_lowercase().as_str(),
                        "0" | "false" | "no" | "off"
                    )
                })
                .unwrap_or(true),
            version: env!("CARGO_PKG_VERSION"),
        }
    }
}
