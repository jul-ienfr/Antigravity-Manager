// 工具函数

pub fn generate_random_id() -> String {
    use rand::Rng;
    rand::thread_rng()
        .sample_iter(&rand::distributions::Alphanumeric)
        .take(8)
        .map(char::from)
        .collect()
}

/// 根据模型名称推测功能类型
// 注意：此函数已弃用，请改用 mappers::common_utils::resolve_request_config
pub fn _deprecated_infer_quota_group(model: &str) -> String {
    if model.to_lowercase().starts_with("claude") {
        "claude".to_string()
    } else {
        "gemini".to_string()
    }
}

/// Clean up the anthropic-beta header from client requests to remove invalid values 
/// like "afk-mode-2026-01-31" which trigger HTTP 400 Bad Request directly from Anthropic.
/// Additionally merges essential internal beta features so the client doesn't need to specify them.
pub fn sanitize_anthropic_beta(client_header: Option<&str>) -> String {
    let mut features: std::collections::HashSet<String> = std::collections::HashSet::new();
    
    // Add default required proxy features
    features.insert("claude-code-20250219".to_string());
    
    // Parse client features if provided
    if let Some(header) = client_header {
        for feature in header.split(',') {
            let trimmed = feature.trim();
            if !trimmed.is_empty() && !trimmed.contains("afk-mode") {
                features.insert(trimmed.to_string());
            }
        }
    }
    
    let mut features_vec: Vec<String> = features.into_iter().collect();
    // Sort for deterministic results
    features_vec.sort(); 
    features_vec.join(",")
}
