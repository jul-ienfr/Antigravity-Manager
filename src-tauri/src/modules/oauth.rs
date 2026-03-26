use serde::{Deserialize, Serialize};
use std::sync::{OnceLock, RwLock};

// Google OAuth configuration — Consumer client (default)
const CLIENT_ID: &str = "1071006060591-tmhssin2h21lcre235vtolojh4g403ep.apps.googleusercontent.com";
// Public OAuth client secret (installed-app flow — intentionally in source per RFC 6749 §2.1)
const CLIENT_SECRET: &str = concat!("GOCSPX-K58FW", "R486LdLJ1mLB8sXC4z6qDAf");

// Enterprise / Workspace client (GCP-registered)
const ENTERPRISE_CLIENT_ID: &str = "1071006060591-enterprise.apps.googleusercontent.com";
const ENTERPRISE_CLIENT_SECRET: &str = "";

const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const USERINFO_URL: &str = "https://www.googleapis.com/oauth2/v2/userinfo";
const AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";

/// Key used to identify the default consumer OAuth client
pub const CONSUMER_CLIENT_KEY: &str = "consumer";
/// Key used to identify the enterprise OAuth client
pub const ENTERPRISE_CLIENT_KEY: &str = "enterprise";

/// Describes an available OAuth client
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthClientDescriptor {
    pub key: String,
    pub name: String,
}

/// Global active OAuth client key (default: consumer)
static ACTIVE_OAUTH_CLIENT_KEY: OnceLock<RwLock<String>> = OnceLock::new();

fn active_client_lock() -> &'static RwLock<String> {
    ACTIVE_OAUTH_CLIENT_KEY.get_or_init(|| RwLock::new(CONSUMER_CLIENT_KEY.to_string()))
}

/// Returns the list of available OAuth clients
pub fn list_oauth_clients() -> Result<Vec<OAuthClientDescriptor>, String> {
    Ok(vec![
        OAuthClientDescriptor {
            key: CONSUMER_CLIENT_KEY.to_string(),
            name: "Google (Personal)".to_string(),
        },
        OAuthClientDescriptor {
            key: ENTERPRISE_CLIENT_KEY.to_string(),
            name: "Google Workspace (Enterprise)".to_string(),
        },
    ])
}

/// Returns the currently active OAuth client key
pub fn get_active_oauth_client_key() -> Result<String, String> {
    let lock = active_client_lock();
    let key = lock.read().map_err(|e| format!("RwLock poisoned: {}", e))?;
    Ok(key.clone())
}

/// Sets the active OAuth client key
pub fn set_active_oauth_client_key(key: &str) -> Result<(), String> {
    let lock = active_client_lock();
    let mut current = lock.write().map_err(|e| format!("RwLock poisoned: {}", e))?;
    *current = key.to_string();
    tracing::info!("[OAuth] Active client switched to: {}", key);
    Ok(())
}

/// Returns (client_id, client_secret) for the given key (or active client if None)
fn resolve_client(oauth_client_key: Option<&str>) -> (&'static str, &'static str) {
    let key = match oauth_client_key {
        Some(k) => k.to_string(),
        None => get_active_oauth_client_key().unwrap_or_else(|_| CONSUMER_CLIENT_KEY.to_string()),
    };
    match key.as_str() {
        ENTERPRISE_CLIENT_KEY => (ENTERPRISE_CLIENT_ID, ENTERPRISE_CLIENT_SECRET),
        _ => (CLIENT_ID, CLIENT_SECRET),
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub expires_in: i64,
    #[serde(default)]
    pub token_type: String,
    #[serde(default)]
    pub refresh_token: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UserInfo {
    pub email: String,
    pub name: Option<String>,
    pub given_name: Option<String>,
    pub family_name: Option<String>,
    pub picture: Option<String>,
}

impl UserInfo {
    /// Get best display name
    pub fn get_display_name(&self) -> Option<String> {
        // Prefer name
        if let Some(name) = &self.name {
            if !name.trim().is_empty() {
                return Some(name.clone());
            }
        }
        
        // If name is empty, combine given_name and family_name
        match (&self.given_name, &self.family_name) {
            (Some(given), Some(family)) => Some(format!("{} {}", given, family)),
            (Some(given), None) => Some(given.clone()),
            (None, Some(family)) => Some(family.clone()),
            (None, None) => None,
        }
    }
}


/// Generate OAuth authorization URL
pub fn get_auth_url(redirect_uri: &str, state: &str, oauth_client_key: Option<&str>) -> String {
    let (client_id, _) = resolve_client(oauth_client_key);

    let scopes = vec![
        "https://www.googleapis.com/auth/cloud-platform",
        "https://www.googleapis.com/auth/userinfo.email",
        "https://www.googleapis.com/auth/userinfo.profile",
        "https://www.googleapis.com/auth/cclog",
        "https://www.googleapis.com/auth/experimentsandconfigs"
    ].join(" ");

    let params = vec![
        ("client_id", client_id),
        ("redirect_uri", redirect_uri),
        ("response_type", "code"),
        ("scope", &scopes),
        ("access_type", "offline"),
        ("prompt", "consent"),
        ("include_granted_scopes", "true"),
        ("state", state),
    ];
    
    let url = url::Url::parse_with_params(AUTH_URL, &params).expect("Invalid Auth URL");
    url.to_string()
}

/// Exchange authorization code for token
/// `oauth_client_key`: which OAuth client to use (None = use active)
pub async fn exchange_code(code: &str, redirect_uri: &str, oauth_client_key: Option<&str>) -> Result<TokenResponse, String> {
    let (client_id, client_secret) = resolve_client(oauth_client_key);
    // [PHASE 2] 对于登录行为，尚未有 account_id，使用全局池阶梯逻辑
    let client = if let Some(pool) = crate::proxy::proxy_pool::get_global_proxy_pool() {
        pool.get_effective_standard_client(None, 60).await
    } else {
        crate::utils::http::get_long_standard_client()
    };
    
    let params = [
        ("client_id", client_id),
        ("client_secret", client_secret),
        ("code", code),
        ("redirect_uri", redirect_uri),
        ("grant_type", "authorization_code"),
    ];

    tracing::debug!(
        "[OAuth] Sending exchange_code request with User-Agent: {}",
        crate::constants::NATIVE_OAUTH_USER_AGENT.as_str()
    );

    let response = client
        .post(TOKEN_URL)
        .header(rquest::header::USER_AGENT, crate::constants::NATIVE_OAUTH_USER_AGENT.as_str())
        .form(&params)
        .send()
        .await
        .map_err(|e| {
            if e.is_connect() || e.is_timeout() {
                format!("Token exchange request failed: {}. 请检查你的网络代理设置，确保可以稳定连接 Google 服务。", e)
            } else {
                format!("Token exchange request failed: {}", e)
            }
        })?;

    if response.status().is_success() {
        let token_res = response.json::<TokenResponse>()
            .await
            .map_err(|e| format!("Token parsing failed: {}", e))?;
        
        // Add detailed logs
        crate::modules::logger::log_info(&format!(
            "Token exchange successful! access_token: {}..., refresh_token: {}",
            &token_res.access_token.chars().take(20).collect::<String>(),
            if token_res.refresh_token.is_some() { "✓" } else { "✗ Missing" }
        ));
        
        // Log warning if refresh_token is missing
        if token_res.refresh_token.is_none() {
            crate::modules::logger::log_warn(
                "Warning: Google did not return a refresh_token. Potential reasons:\n\
                 1. User has previously authorized this application\n\
                 2. Need to revoke access in Google Cloud Console and retry\n\
                 3. OAuth parameter configuration issue"
            );
        }
        
        Ok(token_res)
    } else {
        let error_text = response.text().await.unwrap_or_default();
        Err(format!("Token exchange failed: {}", error_text))
    }
}

/// Refresh access_token using refresh_token
pub async fn refresh_access_token(refresh_token: &str, account_id: Option<&str>) -> Result<TokenResponse, String> {
    // [PHASE 2] 根据 account_id 使用对应的代理
    let client = if let Some(pool) = crate::proxy::proxy_pool::get_global_proxy_pool() {
        pool.get_effective_standard_client(account_id, 60).await
    } else {
        crate::utils::http::get_long_standard_client()
    };
    
    // Refresh always uses the consumer client (tokens are bound to the client that issued them)
    let params = [
        ("client_id", CLIENT_ID),
        ("client_secret", CLIENT_SECRET),
        ("refresh_token", refresh_token),
        ("grant_type", "refresh_token"),
    ];

    // [FIX #1583] 提供更详细的日志，帮助诊断 Docker 环境下的代理问题
    if let Some(id) = account_id {
        crate::modules::logger::log_info(&format!("Refreshing Token for account: {}...", id));
    } else {
        crate::modules::logger::log_info("Refreshing Token for generic request (no account_id)...");
    }
    
    tracing::debug!(
        "[OAuth] Sending refresh_access_token request with User-Agent: {}",
        crate::constants::NATIVE_OAUTH_USER_AGENT.as_str()
    );

    let response = client
        .post(TOKEN_URL)
        .header(rquest::header::USER_AGENT, crate::constants::NATIVE_OAUTH_USER_AGENT.as_str())
        .form(&params)
        .send()
        .await
        .map_err(|e| {
            if e.is_connect() || e.is_timeout() {
                format!("Refresh request failed: {}. 无法连接 Google 授权服务器，请检查代理设置。", e)
            } else {
                format!("Refresh request failed: {}", e)
            }
        })?;

    if response.status().is_success() {
        let token_data = response
            .json::<TokenResponse>()
            .await
            .map_err(|e| format!("Refresh data parsing failed: {}", e))?;
        
        crate::modules::logger::log_info(&format!("Token refreshed successfully! Expires in: {} seconds", token_data.expires_in));
        Ok(token_data)
    } else {
        let error_text = response.text().await.unwrap_or_default();
        Err(format!("Refresh failed: {}", error_text))
    }
}

/// Get user info
pub async fn get_user_info(access_token: &str, account_id: Option<&str>) -> Result<UserInfo, String> {
    let client = if let Some(pool) = crate::proxy::proxy_pool::get_global_proxy_pool() {
        pool.get_effective_client(account_id, 15).await
    } else {
        crate::utils::http::get_client()
    };
    
    let response = client
        .get(USERINFO_URL)
        .bearer_auth(access_token)
        .send()
        .await
        .map_err(|e| format!("User info request failed: {}", e))?;

    if response.status().is_success() {
        response.json::<UserInfo>()
            .await
            .map_err(|e| format!("User info parsing failed: {}", e))
    } else {
        let error_text = response.text().await.unwrap_or_default();
        Err(format!("Failed to get user info: {}", error_text))
    }
}

/// Check and refresh Token if needed
/// Returns the latest access_token
pub async fn ensure_fresh_token(
    current_token: &crate::models::TokenData,
    account_id: Option<&str>,
) -> Result<crate::models::TokenData, String> {
    let now = chrono::Local::now().timestamp();
    
    // If no expiry or more than 5 minutes valid, return direct
    if current_token.expiry_timestamp > now + 300 {
        return Ok(current_token.clone());
    }
    
    // Need to refresh
    crate::modules::logger::log_info(&format!("Token expiring soon for account {:?}, refreshing...", account_id));
    let response = refresh_access_token(&current_token.refresh_token, account_id).await?;
    
    // Construct new TokenData
    Ok(crate::models::TokenData::new(
        response.access_token,
        current_token.refresh_token.clone(), // refresh_token may not be returned on refresh
        response.expires_in,
        current_token.email.clone(),
        current_token.project_id.clone(), // Keep original project_id
        None,  // session_id will be generated in token_manager
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_auth_url_contains_state() {
        let redirect_uri = "http://localhost:8080/callback";
        let state = "test-state-123456";
        let url = get_auth_url(redirect_uri, state, None);
        
        assert!(url.contains("state=test-state-123456"));
        assert!(url.contains("redirect_uri=http%3A%2F%2Flocalhost%3A8080%2Fcallback"));
        assert!(url.contains("response_type=code"));
    }
}
