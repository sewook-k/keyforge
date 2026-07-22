use crate::model::{Action, CURRENT_SCHEMA_VERSION, ProfileScope, Settings, Trigger};
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error, PartialEq)]
pub enum ValidationError {
    #[error("unsupported schema version {0}")]
    Schema(u32),
    #[error("profile {0} has an empty name")]
    EmptyProfileName(Uuid),
    #[error("profile {0} requires at least one non-empty condition")]
    EmptyConditions(Uuid),
    #[error("profile {0} uses an application or device scope that is not supported by this build")]
    UnsupportedScope(Uuid),
    #[error("profile {profile} contains duplicate rule id {rule}")]
    DuplicateRule { profile: Uuid, rule: Uuid },
    #[error("rule {0} has an empty keyboard chord")]
    EmptyChord(Uuid),
    #[error("rule {0} has an unsafe execution limit")]
    UnsafeLimit(Uuid),
    #[error("profile id {0} is duplicated")]
    DuplicateProfile(Uuid),
    #[error("the emergency stop chord must contain at least one unique key")]
    InvalidEmergencyStop,
    #[error("rule {0} has an empty output chord")]
    EmptyOutputChord(Uuid),
}

pub fn validate(settings: &Settings) -> Result<(), ValidationError> {
    if settings.schema_version != CURRENT_SCHEMA_VERSION {
        return Err(ValidationError::Schema(settings.schema_version));
    }
    if settings.engine.emergency_stop.is_empty()
        || settings
            .engine
            .emergency_stop
            .iter()
            .any(|key| key.trim().is_empty())
    {
        return Err(ValidationError::InvalidEmergencyStop);
    }

    let mut profile_ids = std::collections::HashSet::new();
    for profile in &settings.profiles {
        if !profile_ids.insert(profile.id) {
            return Err(ValidationError::DuplicateProfile(profile.id));
        }
        if profile.name.trim().is_empty() {
            return Err(ValidationError::EmptyProfileName(profile.id));
        }
        if !matches!(&profile.scope, ProfileScope::Global) {
            return Err(ValidationError::UnsupportedScope(profile.id));
        }
        if let ProfileScope::Application { conditions }
        | ProfileScope::Device { conditions }
        | ProfileScope::Combined { conditions } = &profile.scope
            && (conditions.conditions.is_empty()
                || conditions
                    .conditions
                    .iter()
                    .any(|condition| condition.value().trim().is_empty()))
        {
            return Err(ValidationError::EmptyConditions(profile.id));
        }

        let mut ids = std::collections::HashSet::new();
        for rule in &profile.rules {
            if !ids.insert(rule.id) {
                return Err(ValidationError::DuplicateRule {
                    profile: profile.id,
                    rule: rule.id,
                });
            }
            if let Trigger::Keyboard { chord, .. } = &rule.trigger
                && (chord.is_empty() || chord.iter().any(|key| key.trim().is_empty()))
            {
                return Err(ValidationError::EmptyChord(rule.id));
            }
            if rule.options.max_executions == 0
                || rule.options.max_executions > settings.engine.max_rule_executions
                || rule.options.timeout_ms == 0
                || rule.options.timeout_ms > settings.engine.max_rule_duration_ms
            {
                return Err(ValidationError::UnsafeLimit(rule.id));
            }
            if let Action::SendKeys { chord } = &rule.action
                && (chord.is_empty() || chord.iter().any(|key| key.trim().is_empty()))
            {
                return Err(ValidationError::EmptyOutputChord(rule.id));
            }
        }
    }
    Ok(())
}
