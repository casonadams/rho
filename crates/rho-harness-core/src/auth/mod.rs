//! Core credential models, authentication traits, and UI-neutral login callbacks.

use crate::error::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelectOption {
    pub id: String,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

impl SelectOption {
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            description: None,
        }
    }

    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StoredCredential {
    #[serde(rename = "api_key")]
    ApiKey {
        key: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        env: Option<HashMap<String, String>>,
    },
    #[serde(rename = "oauth")]
    OAuth {
        #[serde(rename = "access")]
        access_token: String,
        #[serde(rename = "refresh", default, skip_serializing_if = "Option::is_none")]
        refresh_token: Option<String>,
        #[serde(rename = "expires", default, skip_serializing_if = "Option::is_none")]
        expires_at_ms: Option<i64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        account_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        account_email: Option<String>,
    },
}

impl StoredCredential {
    pub fn api_key(key: impl Into<String>) -> Self {
        Self::ApiKey {
            key: key.into(),
            env: None,
        }
    }

    pub fn oauth(access_token: impl Into<String>, refresh_token: Option<String>, expires_at_ms: Option<i64>) -> Self {
        Self::OAuth {
            access_token: access_token.into(),
            refresh_token,
            expires_at_ms,
            account_id: None,
            account_email: None,
        }
    }

    pub fn raw_secret(&self) -> &str {
        match self {
            Self::ApiKey { key, .. } => key,
            Self::OAuth { access_token, .. } => access_token,
        }
    }

    pub fn is_expired(&self, threshold_secs: i64) -> bool {
        match self {
            Self::ApiKey { .. } => false,
            Self::OAuth {
                expires_at_ms: Some(exp),
                ..
            } => {
                let now_ms = chrono::Utc::now().timestamp_millis();
                let threshold_ms = threshold_secs * 1000;
                now_ms + threshold_ms >= *exp
            }
            Self::OAuth {
                expires_at_ms: None, ..
            } => false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceCodeInfo<'a> {
    pub user_code: &'a str,
    pub verification_uri: &'a str,
    pub interval_secs: u64,
    pub expires_in_secs: u64,
}

/// UI-agnostic callbacks for OAuth and interactive authentication flows.
#[async_trait]
pub trait OAuthLoginCallbacks: Send + Sync {
    async fn on_auth_url(&self, url: &str, instructions: Option<&str>) -> Result<()>;
    async fn on_device_code(&self, info: &DeviceCodeInfo<'_>) -> Result<()>;
    async fn on_prompt(&self, message: &str, secret: bool) -> Result<String>;
    async fn on_select(&self, message: &str, options: &[SelectOption]) -> Result<Option<String>>;
    async fn on_progress(&self, message: &str) -> Result<()>;
}

#[cfg(test)]
mod tests;
