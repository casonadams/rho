//! Cloud Code Assist discovery: project id resolution and the live model
//! catalog via `v1internal` metadata endpoints.

use super::http::post_metadata;

pub(super) fn extract_project_id(value: &serde_json::Value) -> Option<String> {
    let direct = value
        .get("antigravityProjectId")
        .or_else(|| value.get("projectId"))
        .or_else(|| value.get("backendProjectId"))
        .or_else(|| value.get("cloudaicompanionProject"));
    if let Some(id) = direct.and_then(|v| v.as_str()) {
        return Some(id.to_string());
    }
    for key in ["projects", "projectIds", "cloudaicompanionProjects"] {
        if let Some(items) = value.get(key).and_then(|v| v.as_array()) {
            for item in items {
                if let Some(id) = item.as_str() {
                    return Some(id.to_string());
                }
                if let Some(found) = extract_project_id(item) {
                    return Some(found);
                }
            }
        }
    }
    None
}

/// Discover the Cloud Code Assist project id for the signed-in account.
pub async fn load_project_id(token: &str) -> Option<String> {
    let body = serde_json::json!({
        "metadata": {
            "ideType": "ANTIGRAVITY",
            "platform": "PLATFORM_UNSPECIFIED",
            "pluginType": "GEMINI"
        }
    });
    if let Some(project) = post_metadata("/v1internal:loadCodeAssist", token, body)
        .await
        .as_ref()
        .and_then(extract_project_id)
    {
        return Some(project);
    }
    post_metadata("/v1internal:listCloudAICompanionProjects", token, serde_json::json!({}))
        .await
        .as_ref()
        .and_then(extract_project_id)
}

/// Runtime models selectable in rho (pi parity filters: gemini-/claude-/
/// gpt-oss- prefixed, no chat/tab/image entries).
pub fn is_selectable_runtime_model(id: &str) -> bool {
    let selectable = id.starts_with("gemini-") || id.starts_with("claude-") || id.starts_with("gpt-oss-");
    selectable && !id.contains(char::is_whitespace) && !id.contains("image") && !id.starts_with("MODEL_")
}

/// Live model catalog via `v1internal:fetchAvailableModels`.
pub async fn discover_models(token: &str, project_id: &str) -> Option<Vec<String>> {
    let response = post_metadata(
        "/v1internal:fetchAvailableModels",
        token,
        serde_json::json!({ "project": project_id }),
    )
    .await?;
    let models = response.get("models")?.as_object()?;
    let mut ids: Vec<String> = models
        .keys()
        .filter(|id| is_selectable_runtime_model(id))
        .cloned()
        .collect();
    if ids.is_empty() {
        return None;
    }
    ids.sort();
    Some(ids)
}
