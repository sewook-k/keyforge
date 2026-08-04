use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ActionResult {
    pub action_id: Uuid,
    pub action_type: String,
    pub status: ActionStatus,
    pub message: String,
    pub revision: Option<u64>,
    pub timestamp: DateTime<Utc>,
    pub recovery: Option<Recovery>,
    pub details: Option<Value>,
}

impl ActionResult {
    pub fn success(
        action_type: impl Into<String>,
        message: impl Into<String>,
        revision: Option<u64>,
    ) -> Self {
        Self {
            action_id: Uuid::new_v4(),
            action_type: action_type.into(),
            status: ActionStatus::Success,
            message: message.into(),
            revision,
            timestamp: Utc::now(),
            recovery: None,
            details: None,
        }
    }

    pub fn error(
        action_type: impl Into<String>,
        message: impl Into<String>,
        recovery: Recovery,
    ) -> Self {
        Self {
            action_id: Uuid::new_v4(),
            action_type: action_type.into(),
            status: ActionStatus::Error,
            message: message.into(),
            revision: None,
            timestamp: Utc::now(),
            recovery: Some(recovery),
            details: None,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ActionStatus {
    Success,
    Warning,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Recovery {
    pub attempted: bool,
    pub succeeded: Option<bool>,
    pub message: Option<String>,
    #[serde(default)]
    pub actions: Vec<RecoveryAction>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryAction {
    Retry,
    RestoreBackup,
    OpenLogs,
}
