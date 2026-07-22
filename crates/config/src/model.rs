use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const CURRENT_SCHEMA_VERSION: u32 = 3;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub schema_version: u32,
    pub revision: u64,
    pub updated_at: DateTime<Utc>,
    #[serde(default)]
    pub profiles: Vec<Profile>,
    #[serde(default)]
    pub preferences: Preferences,
    #[serde(default)]
    pub engine: EngineSettings,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            schema_version: CURRENT_SCHEMA_VERSION,
            revision: 0,
            updated_at: Utc::now(),
            profiles: vec![Profile::starter()],
            preferences: Preferences::default(),
            engine: EngineSettings::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    pub id: Uuid,
    pub name: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub archived: bool,
    #[serde(default)]
    pub enable_on_startup: bool,
    #[serde(default)]
    pub scope: ProfileScope,
    #[serde(default)]
    pub rules: Vec<Rule>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(default)]
    pub last_run_at: Option<DateTime<Utc>>,
}

impl Profile {
    pub fn new(name: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            enabled: true,
            archived: false,
            enable_on_startup: false,
            scope: ProfileScope::Global,
            rules: Vec::new(),
            created_at: now,
            updated_at: now,
            last_run_at: None,
        }
    }

    fn starter() -> Self {
        let mut profile = Self::new("기본 전역 프로필");
        profile.enabled = false;
        profile
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProfileScope {
    #[default]
    Global,
    Application {
        conditions: ConditionGroup,
    },
    Device {
        conditions: ConditionGroup,
    },
    Combined {
        conditions: ConditionGroup,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ConditionGroup {
    pub operator: ConditionOperator,
    pub conditions: Vec<MatchCondition>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ConditionOperator {
    And,
    Or,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MatchCondition {
    ProcessName {
        operator: TextOperator,
        value: String,
    },
    ExecutablePath {
        operator: TextOperator,
        value: String,
    },
    WindowClass {
        operator: TextOperator,
        value: String,
    },
    DeviceId {
        operator: TextOperator,
        value: String,
    },
}

impl MatchCondition {
    pub fn value(&self) -> &str {
        match self {
            Self::ProcessName { value, .. }
            | Self::ExecutablePath { value, .. }
            | Self::WindowClass { value, .. }
            | Self::DeviceId { value, .. } => value,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TextOperator {
    Equals,
    Contains,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Rule {
    pub id: Uuid,
    pub order: u32,
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub trigger: Trigger,
    pub action: Action,
    #[serde(default)]
    pub options: RuleOptions,
}

impl Rule {
    pub fn key_remap(input: impl Into<String>, output: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            order: 0,
            enabled: true,
            trigger: Trigger::Keyboard {
                chord: vec![input.into()],
                phase: TriggerPhase::Press,
                gesture: TriggerGesture::Single,
            },
            action: Action::SendKeys {
                chord: vec![output.into()],
            },
            options: RuleOptions::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Trigger {
    Keyboard {
        chord: Vec<String>,
        phase: TriggerPhase,
        gesture: TriggerGesture,
    },
    Mouse {
        button: MouseButton,
        phase: TriggerPhase,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TriggerPhase {
    Press,
    Release,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TriggerGesture {
    Single,
    Hold,
    Double,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MouseButton {
    Left,
    Right,
    Middle,
    X1,
    X2,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum Action {
    SendKeys { chord: Vec<String> },
    SendMouse { button: MouseButton },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RuleOptions {
    #[serde(default)]
    pub pass_through_original: bool,
    #[serde(default = "default_true")]
    pub ignore_injected: bool,
    #[serde(default = "default_max_executions")]
    pub max_executions: u32,
    #[serde(default = "default_timeout_ms")]
    pub timeout_ms: u64,
}

impl Default for RuleOptions {
    fn default() -> Self {
        Self {
            pass_through_original: false,
            ignore_injected: true,
            max_executions: default_max_executions(),
            timeout_ms: default_timeout_ms(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Preferences {
    pub theme: Theme,
    pub language: String,
    pub close_to_tray: bool,
    pub start_minimized: bool,
    #[serde(default)]
    pub launch_at_login: bool,
    pub notifications: NotificationLevel,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            theme: Theme::System,
            language: "ko-KR".into(),
            close_to_tray: true,
            start_minimized: false,
            launch_at_login: false,
            notifications: NotificationLevel::All,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NotificationLevel {
    All,
    Warnings,
    Errors,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct EngineSettings {
    pub emergency_stop: Vec<String>,
    #[serde(default = "default_timeout_ms")]
    pub max_rule_duration_ms: u64,
    #[serde(default = "default_max_executions")]
    pub max_rule_executions: u32,
    pub ignore_all_injected_input: bool,
}

impl Default for EngineSettings {
    fn default() -> Self {
        Self {
            emergency_stop: vec!["Control".into(), "Alt".into(), "Pause".into()],
            max_rule_duration_ms: default_timeout_ms(),
            max_rule_executions: default_max_executions(),
            ignore_all_injected_input: true,
        }
    }
}

fn default_true() -> bool {
    true
}
fn default_max_executions() -> u32 {
    1_000
}
fn default_timeout_ms() -> u64 {
    300_000
}
