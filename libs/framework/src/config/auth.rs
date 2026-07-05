use serde::Deserialize;

#[derive(Debug, Deserialize, Clone, Default)]
pub struct AuthConfig {
    #[serde(default)]
    pub access_token_secret: Option<String>,
    #[serde(default)]
    pub access_token_expires_in: Option<u64>,
    #[serde(default)]
    pub refresh_token_secret: Option<String>,
    #[serde(default)]
    pub refresh_token_expires_in: Option<u64>,
    #[serde(default)]
    pub audience: Option<String>,
    #[serde(default)]
    pub issuer: Option<String>,
    #[serde(default)]
    pub client_id: Option<String>,
    #[serde(default)]
    pub client_secret: Option<String>,
}

impl AuthConfig {
    pub fn access_token_secret(&self) -> &str {
        self.access_token_secret
            .as_deref()
            .expect("access_token_secret should have default")
    }

    pub fn access_token_expires_in(&self) -> u64 {
        self.access_token_expires_in
            .expect("access_token_expires_in should have default")
    }

    pub fn refresh_token_secret(&self) -> &str {
        self.refresh_token_secret
            .as_deref()
            .expect("refresh_token_secret should have default")
    }

    pub fn refresh_token_expires_in(&self) -> u64 {
        self.refresh_token_expires_in
            .expect("refresh_token_expires_in should have default")
    }

    pub fn audience(&self) -> &str {
        self.audience
            .as_deref()
            .expect("audience should have default")
    }

    pub fn issuer(&self) -> &str {
        self.issuer.as_deref().expect("issuer should have default")
    }

    pub fn client_id(&self) -> &str {
        self.client_id
            .as_deref()
            .expect("client_id should have default")
    }

    pub fn client_secret(&self) -> &str {
        self.client_secret
            .as_deref()
            .expect("client_secret should have default")
    }
}
