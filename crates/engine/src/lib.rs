use keyforge_config::{Action, ProfileScope, Settings, Trigger, TriggerGesture, TriggerPhase};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventOrigin {
    Physical,
    InjectedSelf,
    InjectedOther,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeyPhase {
    Down,
    Up,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyEvent {
    pub key: String,
    pub phase: KeyPhase,
    pub origin: EventOrigin,
    #[serde(default)]
    pub repeat: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MatchContext {
    pub process_name: Option<String>,
    pub executable_path: Option<String>,
    pub window_class: Option<String>,
    pub device_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Dispatch {
    pub suppress_original: bool,
    pub actions: Vec<DispatchAction>,
    pub emergency_stop: bool,
}

impl Dispatch {
    fn pass() -> Self {
        Self {
            suppress_original: false,
            actions: Vec::new(),
            emergency_stop: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct DispatchAction {
    pub profile_id: Uuid,
    pub rule_id: Uuid,
    pub action: Action,
    pub phase: DispatchActionPhase,
    pub transient_release_inputs: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DispatchActionPhase {
    Invoke,
    Down,
    Up,
}

#[derive(Debug, Error, PartialEq)]
pub enum CompileError {
    #[error("duplicate trigger {trigger} in profiles {first_profile} and {second_profile}")]
    Conflict {
        trigger: String,
        first_profile: Uuid,
        second_profile: Uuid,
    },
    #[error("unsupported trigger gesture in rule {0}")]
    UnsupportedGesture(Uuid),
}

#[derive(Debug, Clone)]
struct CompiledRule {
    profile_id: Uuid,
    rule_id: Uuid,
    chord: Vec<String>,
    phase: KeyPhase,
    action: Action,
    pass_through: bool,
    scope: ProfileScope,
    stateful_remap: bool,
}

#[derive(Debug, Clone)]
pub struct CompiledRules {
    revision: u64,
    rules: Vec<CompiledRule>,
    emergency: BTreeSet<String>,
}

impl CompiledRules {
    pub fn compile(settings: &Settings) -> Result<Self, CompileError> {
        let mut rules = Vec::new();
        let mut seen: HashMap<String, Uuid> = HashMap::new();
        for profile in settings
            .profiles
            .iter()
            .filter(|profile| profile.enabled && !profile.archived)
        {
            for rule in profile.rules.iter().filter(|rule| rule.enabled) {
                {
                    let (chord, phase, gesture) = match &rule.trigger {
                        Trigger::Keyboard {
                            chord,
                            phase,
                            gesture,
                        } => (chord.clone(), *phase, *gesture),
                        Trigger::Mouse { button, phase } => (
                            vec![mouse_key(*button).into()],
                            *phase,
                            TriggerGesture::Single,
                        ),
                    };
                    if gesture != TriggerGesture::Single {
                        return Err(CompileError::UnsupportedGesture(rule.id));
                    }
                    let mut normalized: Vec<_> =
                        chord.iter().map(|key| normalize_key(key)).collect();
                    normalized.sort();
                    normalized.dedup();
                    let phase = match phase {
                        TriggerPhase::Press => KeyPhase::Down,
                        TriggerPhase::Release => KeyPhase::Up,
                    };
                    let stateful_remap = phase == KeyPhase::Down
                        && !rule.options.pass_through_original
                        && normalized.len() == 1
                        && matches!(&rule.action, Action::SendKeys { chord } if chord.len() == 1);
                    let signature = format!(
                        "{}:{phase:?}:{}",
                        scope_signature(&profile.scope),
                        normalized.join("+")
                    );
                    if let Some(previous) = seen.insert(signature.clone(), profile.id) {
                        return Err(CompileError::Conflict {
                            trigger: signature,
                            first_profile: previous,
                            second_profile: profile.id,
                        });
                    }
                    rules.push(CompiledRule {
                        profile_id: profile.id,
                        rule_id: rule.id,
                        chord: normalized,
                        phase,
                        action: rule.action.clone(),
                        pass_through: rule.options.pass_through_original,
                        scope: profile.scope.clone(),
                        stateful_remap,
                    });
                }
            }
        }
        Ok(Self {
            revision: settings.revision,
            rules,
            emergency: settings
                .engine
                .emergency_stop
                .iter()
                .map(|key| normalize_key(key))
                .collect(),
        })
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn len(&self) -> usize {
        self.rules.len()
    }
    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }
}

pub struct RuntimeEngine {
    rules: CompiledRules,
    pressed: BTreeSet<String>,
    paused: bool,
    consumed_inputs: BTreeSet<String>,
    held_outputs: BTreeMap<String, DispatchAction>,
}

impl RuntimeEngine {
    pub fn new(rules: CompiledRules) -> Self {
        Self {
            rules,
            pressed: BTreeSet::new(),
            paused: false,
            consumed_inputs: BTreeSet::new(),
            held_outputs: BTreeMap::new(),
        }
    }

    pub fn replace_rules(&mut self, rules: CompiledRules) {
        self.rules = rules;
    }
    pub fn revision(&self) -> u64 {
        self.rules.revision()
    }
    pub fn set_paused(&mut self, paused: bool) -> Vec<DispatchAction> {
        if self.paused == paused {
            return Vec::new();
        }
        self.paused = paused;
        if paused {
            self.pressed.clear();
            return self.take_held_releases();
        }
        Vec::new()
    }
    pub fn cancel_consumed(&mut self, key: &str) {
        let key = normalize_key(key);
        self.consumed_inputs.remove(&key);
        self.held_outputs.remove(&key);
    }
    pub fn is_paused(&self) -> bool {
        self.paused
    }

    pub fn process(&mut self, event: &KeyEvent, context: &MatchContext) -> Dispatch {
        if event.origin != EventOrigin::Physical {
            return Dispatch::pass();
        }
        let key = normalize_key(&event.key);

        if event.phase == KeyPhase::Up && self.consumed_inputs.remove(&key) {
            self.pressed.remove(&key);
            let actions = self
                .held_outputs
                .remove(&key)
                .map(|mut action| {
                    action.phase = DispatchActionPhase::Up;
                    action
                })
                .into_iter()
                .collect();
            return Dispatch {
                suppress_original: true,
                actions,
                emergency_stop: false,
            };
        }

        if self.paused {
            if event.phase == KeyPhase::Up {
                self.pressed.remove(&key);
            }
            return Dispatch {
                suppress_original: event.phase == KeyPhase::Down
                    && self.consumed_inputs.contains(&key),
                actions: Vec::new(),
                emergency_stop: false,
            };
        }

        if event.phase == KeyPhase::Down {
            if let Some(action) = self.held_outputs.get(&key) {
                return Dispatch {
                    suppress_original: true,
                    actions: vec![action.clone()],
                    emergency_stop: false,
                };
            }
            if self.consumed_inputs.contains(&key) {
                return Dispatch {
                    suppress_original: true,
                    actions: Vec::new(),
                    emergency_stop: false,
                };
            }
        }

        if event.phase == KeyPhase::Down {
            self.pressed.insert(key.clone());
        }

        if event.phase == KeyPhase::Down
            && self
                .rules
                .emergency
                .iter()
                .all(|required| pressed_contains(&self.pressed, required))
        {
            self.paused = true;
            self.pressed.clear();
            return Dispatch {
                suppress_original: true,
                actions: self.take_held_releases(),
                emergency_stop: true,
            };
        }

        for rule in &self.rules.rules {
            if rule.phase != event.phase
                || !rule
                    .chord
                    .iter()
                    .any(|required| key_matches(&key, required))
                || !scope_matches(&rule.scope, context)
            {
                continue;
            }
            if event.phase == KeyPhase::Down
                && rule
                    .chord
                    .iter()
                    .all(|required| pressed_contains(&self.pressed, required))
            {
                let transient_release_inputs = if rule.stateful_remap {
                    Vec::new()
                } else {
                    transient_modifier_releases(
                        &self.pressed,
                        &self.held_outputs,
                        &rule.chord,
                        &rule.action,
                        &key,
                    )
                };
                let action = DispatchAction {
                    profile_id: rule.profile_id,
                    rule_id: rule.rule_id,
                    action: rule.action.clone(),
                    phase: if rule.stateful_remap {
                        DispatchActionPhase::Down
                    } else {
                        DispatchActionPhase::Invoke
                    },
                    transient_release_inputs,
                };
                if !rule.pass_through {
                    self.consumed_inputs.insert(key.clone());
                }
                if rule.stateful_remap {
                    self.held_outputs.insert(key.clone(), action.clone());
                }
                return Dispatch {
                    suppress_original: !rule.pass_through,
                    actions: vec![action],
                    emergency_stop: false,
                };
            }
            if event.phase == KeyPhase::Up
                && rule
                    .chord
                    .iter()
                    .all(|required| pressed_contains(&self.pressed, required))
            {
                self.pressed.remove(&key);
                return Dispatch {
                    suppress_original: !rule.pass_through,
                    actions: vec![DispatchAction {
                        profile_id: rule.profile_id,
                        rule_id: rule.rule_id,
                        action: rule.action.clone(),
                        phase: DispatchActionPhase::Invoke,
                        transient_release_inputs: transient_modifier_releases(
                            &self.pressed,
                            &self.held_outputs,
                            &rule.chord,
                            &rule.action,
                            &key,
                        ),
                    }],
                    emergency_stop: false,
                };
            }
        }
        if event.phase == KeyPhase::Up {
            self.pressed.remove(&key);
        }
        Dispatch::pass()
    }

    fn take_held_releases(&mut self) -> Vec<DispatchAction> {
        std::mem::take(&mut self.held_outputs)
            .into_values()
            .map(|mut action| {
                action.phase = DispatchActionPhase::Up;
                action
            })
            .collect()
    }
}

fn normalize_key(key: &str) -> String {
    let compact = key.trim().to_ascii_lowercase().replace([' ', '-'], "");
    match compact.as_str() {
        "ctrl" => "control".into(),
        "esc" => "escape".into(),
        value => value.into(),
    }
}

fn pressed_contains(pressed: &BTreeSet<String>, required: &str) -> bool {
    if pressed.contains(required) {
        return true;
    }
    match required {
        "control" => pressed.contains("controlleft") || pressed.contains("controlright"),
        "alt" => pressed.contains("altleft") || pressed.contains("altright"),
        "shift" => pressed.contains("shiftleft") || pressed.contains("shiftright"),
        "meta" => pressed.contains("metaleft") || pressed.contains("metaright"),
        _ => false,
    }
}

fn key_matches(actual: &str, required: &str) -> bool {
    actual == required
        || match required {
            "control" => matches!(actual, "controlleft" | "controlright"),
            "alt" => matches!(actual, "altleft" | "altright"),
            "shift" => matches!(actual, "shiftleft" | "shiftright"),
            "meta" => matches!(actual, "metaleft" | "metaright"),
            _ => false,
        }
}

fn keys_overlap(left: &str, right: &str) -> bool {
    key_matches(left, right) || key_matches(right, left)
}

fn is_modifier_key(key: &str) -> bool {
    matches!(
        key,
        "control"
            | "controlleft"
            | "controlright"
            | "alt"
            | "altleft"
            | "altright"
            | "shift"
            | "shiftleft"
            | "shiftright"
            | "meta"
            | "metaleft"
            | "metaright"
    )
}

fn transient_modifier_releases(
    pressed: &BTreeSet<String>,
    held_outputs: &BTreeMap<String, DispatchAction>,
    trigger_chord: &[String],
    action: &Action,
    event_key: &str,
) -> Vec<String> {
    let Action::SendKeys { chord } = action else {
        return Vec::new();
    };
    let output: Vec<_> = chord.iter().map(|key| normalize_key(key)).collect();
    let mut releases = Vec::new();
    for actual in pressed.iter() {
        if actual == event_key || !is_modifier_key(actual) {
            continue;
        }
        if !trigger_chord
            .iter()
            .any(|required| key_matches(actual, required))
        {
            continue;
        }
        let active_keys = held_outputs
            .get(actual)
            .and_then(|dispatch| match &dispatch.action {
                Action::SendKeys { chord } => Some(
                    chord
                        .iter()
                        .map(|key| normalize_key(key))
                        .collect::<Vec<_>>(),
                ),
                _ => None,
            })
            .unwrap_or_else(|| vec![actual.clone()]);
        for active_key in active_keys {
            if output
                .iter()
                .any(|output_key| keys_overlap(&active_key, output_key))
            {
                continue;
            }
            releases.push(active_key);
        }
    }
    releases.sort();
    releases.dedup();
    releases
}

fn mouse_key(button: keyforge_config::MouseButton) -> &'static str {
    match button {
        keyforge_config::MouseButton::Left => "mouseleft",
        keyforge_config::MouseButton::Right => "mouseright",
        keyforge_config::MouseButton::Middle => "mousemiddle",
        keyforge_config::MouseButton::X1 => "mousex1",
        keyforge_config::MouseButton::X2 => "mousex2",
    }
}

fn scope_signature(scope: &ProfileScope) -> &'static str {
    match scope {
        ProfileScope::Global => "global",
        ProfileScope::Application { .. } => "application",
        ProfileScope::Device { .. } => "device",
        ProfileScope::Combined { .. } => "combined",
    }
}

fn scope_matches(scope: &ProfileScope, context: &MatchContext) -> bool {
    use keyforge_config::{ConditionOperator, MatchCondition, TextOperator};
    let conditions = match scope {
        ProfileScope::Global => return true,
        ProfileScope::Application { conditions }
        | ProfileScope::Device { conditions }
        | ProfileScope::Combined { conditions } => conditions,
    };
    let evaluate = |condition: &MatchCondition| {
        let (actual, operator, expected) = match condition {
            MatchCondition::ProcessName { operator, value } => {
                (context.process_name.as_deref(), operator, value)
            }
            MatchCondition::ExecutablePath { operator, value } => {
                (context.executable_path.as_deref(), operator, value)
            }
            MatchCondition::WindowClass { operator, value } => {
                (context.window_class.as_deref(), operator, value)
            }
            MatchCondition::DeviceId { operator, value } => {
                (context.device_id.as_deref(), operator, value)
            }
        };
        let Some(actual) = actual else {
            return false;
        };
        match operator {
            TextOperator::Equals => actual.eq_ignore_ascii_case(expected),
            TextOperator::Contains => actual
                .to_ascii_lowercase()
                .contains(&expected.to_ascii_lowercase()),
        }
    };
    match conditions.operator {
        ConditionOperator::And => conditions.conditions.iter().all(evaluate),
        ConditionOperator::Or => conditions.conditions.iter().any(evaluate),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use keyforge_config::{Profile, Rule};

    fn settings_with_remap() -> Settings {
        let mut settings = Settings::default();
        let mut profile = Profile::new("global");
        profile.rules.push(Rule::key_remap("A", "B"));
        settings.profiles = vec![profile];
        settings
    }

    #[test]
    fn injected_input_is_never_processed() {
        let rules = CompiledRules::compile(&settings_with_remap()).unwrap();
        let mut engine = RuntimeEngine::new(rules);
        let dispatch = engine.process(
            &KeyEvent {
                key: "A".into(),
                phase: KeyPhase::Down,
                origin: EventOrigin::InjectedSelf,
                repeat: false,
            },
            &MatchContext::default(),
        );
        assert!(!dispatch.suppress_original);
        assert!(dispatch.actions.is_empty());
    }

    #[test]
    fn global_key_remap_matches_without_app_context() {
        let rules = CompiledRules::compile(&settings_with_remap()).unwrap();
        let mut engine = RuntimeEngine::new(rules);
        let dispatch = engine.process(
            &KeyEvent {
                key: "A".into(),
                phase: KeyPhase::Down,
                origin: EventOrigin::Physical,
                repeat: false,
            },
            &MatchContext::default(),
        );
        assert!(dispatch.suppress_original);
        assert!(
            matches!(&dispatch.actions[0].action, Action::SendKeys { chord } if chord == &vec!["B"])
        );
    }

    #[test]
    fn single_modifier_remap_holds_output_until_source_key_up() {
        let mut settings = Settings::default();
        let mut profile = Profile::new("modifier remap");
        profile
            .rules
            .push(Rule::key_remap("ControlLeft", "MetaLeft"));
        settings.profiles = vec![profile];

        let rules = CompiledRules::compile(&settings).unwrap();
        let mut engine = RuntimeEngine::new(rules);
        let down = engine.process(
            &KeyEvent {
                key: "ControlLeft".into(),
                phase: KeyPhase::Down,
                origin: EventOrigin::Physical,
                repeat: false,
            },
            &MatchContext::default(),
        );

        assert!(down.suppress_original);
        assert_eq!(down.actions.len(), 1);
        assert_eq!(down.actions[0].phase, DispatchActionPhase::Down);
        assert!(
            matches!(&down.actions[0].action, Action::SendKeys { chord } if chord == &vec!["MetaLeft"])
        );

        let up = engine.process(
            &KeyEvent {
                key: "ControlLeft".into(),
                phase: KeyPhase::Up,
                origin: EventOrigin::Physical,
                repeat: false,
            },
            &MatchContext::default(),
        );

        assert!(up.suppress_original);
        assert_eq!(up.actions.len(), 1);
        assert_eq!(up.actions[0].phase, DispatchActionPhase::Up);
        assert!(
            matches!(&up.actions[0].action, Action::SendKeys { chord } if chord == &vec!["MetaLeft"])
        );
    }

    #[test]
    fn modifier_cycle_does_not_consume_the_next_ordinary_key() {
        let mut settings = Settings::default();
        let mut profile = Profile::new("three-way modifier cycle");
        profile
            .rules
            .push(Rule::key_remap("AltLeft", "ControlLeft"));
        profile
            .rules
            .push(Rule::key_remap("ControlLeft", "MetaLeft"));
        profile.rules.push(Rule::key_remap("MetaLeft", "AltLeft"));
        settings.profiles = vec![profile];

        let rules = CompiledRules::compile(&settings).unwrap();
        let mut engine = RuntimeEngine::new(rules);

        let control_down = engine.process(
            &KeyEvent {
                key: "ControlLeft".into(),
                phase: KeyPhase::Down,
                origin: EventOrigin::Physical,
                repeat: false,
            },
            &MatchContext::default(),
        );
        assert!(control_down.suppress_original);
        assert_eq!(control_down.actions[0].phase, DispatchActionPhase::Down);
        assert!(
            matches!(&control_down.actions[0].action, Action::SendKeys { chord } if chord == &vec!["MetaLeft"])
        );

        let ordinary_down = engine.process(
            &KeyEvent {
                key: "E".into(),
                phase: KeyPhase::Down,
                origin: EventOrigin::Physical,
                repeat: false,
            },
            &MatchContext::default(),
        );
        assert!(!ordinary_down.suppress_original);
        assert!(ordinary_down.actions.is_empty());

        let ordinary_up = engine.process(
            &KeyEvent {
                key: "E".into(),
                phase: KeyPhase::Up,
                origin: EventOrigin::Physical,
                repeat: false,
            },
            &MatchContext::default(),
        );
        assert!(!ordinary_up.suppress_original);
        assert!(ordinary_up.actions.is_empty());

        let control_up = engine.process(
            &KeyEvent {
                key: "ControlLeft".into(),
                phase: KeyPhase::Up,
                origin: EventOrigin::Physical,
                repeat: false,
            },
            &MatchContext::default(),
        );
        assert!(control_up.suppress_original);
        assert_eq!(control_up.actions[0].phase, DispatchActionPhase::Up);
        assert!(
            matches!(&control_up.actions[0].action, Action::SendKeys { chord } if chord == &vec!["MetaLeft"])
        );
    }

    #[test]
    fn generic_modifier_trigger_matches_the_current_side_specific_key() {
        let mut settings = Settings::default();
        let mut profile = Profile::new("generic modifier");
        profile.rules.push(Rule::key_remap("Control", "F24"));
        settings.profiles = vec![profile];

        let rules = CompiledRules::compile(&settings).unwrap();
        let mut engine = RuntimeEngine::new(rules);
        let dispatch = engine.process(
            &KeyEvent {
                key: "ControlLeft".into(),
                phase: KeyPhase::Down,
                origin: EventOrigin::Physical,
                repeat: false,
            },
            &MatchContext::default(),
        );

        assert!(dispatch.suppress_original);
        assert!(
            matches!(&dispatch.actions[0].action, Action::SendKeys { chord } if chord == &vec!["F24"])
        );
    }

    #[test]
    fn pausing_releases_a_held_modifier_remap() {
        let mut settings = Settings::default();
        let mut profile = Profile::new("modifier remap");
        profile
            .rules
            .push(Rule::key_remap("ControlLeft", "MetaLeft"));
        settings.profiles = vec![profile];

        let rules = CompiledRules::compile(&settings).unwrap();
        let mut engine = RuntimeEngine::new(rules);
        engine.process(
            &KeyEvent {
                key: "ControlLeft".into(),
                phase: KeyPhase::Down,
                origin: EventOrigin::Physical,
                repeat: false,
            },
            &MatchContext::default(),
        );

        let releases = engine.set_paused(true);
        assert_eq!(releases.len(), 1);
        assert_eq!(releases[0].phase, DispatchActionPhase::Up);
        assert!(
            matches!(&releases[0].action, Action::SendKeys { chord } if chord == &vec!["MetaLeft"])
        );

        let source_up = engine.process(
            &KeyEvent {
                key: "ControlLeft".into(),
                phase: KeyPhase::Up,
                origin: EventOrigin::Physical,
                repeat: false,
            },
            &MatchContext::default(),
        );
        assert!(source_up.suppress_original);
        assert!(source_up.actions.is_empty());
    }

    #[test]
    fn emergency_stop_wins_before_rules() {
        let rules = CompiledRules::compile(&settings_with_remap()).unwrap();
        let mut engine = RuntimeEngine::new(rules);
        for key in ["ControlLeft", "AltRight"] {
            engine.process(
                &KeyEvent {
                    key: key.into(),
                    phase: KeyPhase::Down,
                    origin: EventOrigin::Physical,
                    repeat: false,
                },
                &MatchContext::default(),
            );
        }
        let dispatch = engine.process(
            &KeyEvent {
                key: "Pause".into(),
                phase: KeyPhase::Down,
                origin: EventOrigin::Physical,
                repeat: false,
            },
            &MatchContext::default(),
        );
        assert!(dispatch.emergency_stop);
        assert!(engine.is_paused());
    }

    #[test]
    fn side_specific_modifier_rule_does_not_match_the_other_side() {
        let mut settings = Settings::default();
        let mut profile = Profile::new("side-specific");
        let mut rule = Rule::key_remap("K", "F24");
        rule.trigger = Trigger::Keyboard {
            chord: vec!["ControlLeft".into(), "K".into()],
            phase: TriggerPhase::Press,
            gesture: TriggerGesture::Single,
        };
        profile.rules.push(rule);
        settings.profiles = vec![profile];

        let compiled = CompiledRules::compile(&settings).unwrap();
        let mut right_engine = RuntimeEngine::new(compiled.clone());
        for key in ["ControlRight", "K"] {
            let dispatch = right_engine.process(
                &KeyEvent {
                    key: key.into(),
                    phase: KeyPhase::Down,
                    origin: EventOrigin::Physical,
                    repeat: false,
                },
                &MatchContext::default(),
            );
            assert!(dispatch.actions.is_empty());
        }

        let mut left_engine = RuntimeEngine::new(compiled);
        left_engine.process(
            &KeyEvent {
                key: "ControlLeft".into(),
                phase: KeyPhase::Down,
                origin: EventOrigin::Physical,
                repeat: false,
            },
            &MatchContext::default(),
        );
        let dispatch = left_engine.process(
            &KeyEvent {
                key: "K".into(),
                phase: KeyPhase::Down,
                origin: EventOrigin::Physical,
                repeat: false,
            },
            &MatchContext::default(),
        );
        assert!(matches!(
            &dispatch.actions[0].action,
            Action::SendKeys { chord } if chord == &vec!["F24"]
        ));
    }

    #[test]
    fn multi_key_shortcut_releases_only_source_modifiers_not_needed_by_output() {
        let mut settings = Settings::default();
        let mut profile = Profile::new("multi-key shortcut");
        let mut rule = Rule::key_remap("S", "4");
        rule.trigger = Trigger::Keyboard {
            chord: vec!["MetaLeft".into(), "ShiftLeft".into(), "S".into()],
            phase: TriggerPhase::Press,
            gesture: TriggerGesture::Single,
        };
        rule.action = Action::SendKeys {
            chord: vec!["ControlLeft".into(), "ShiftLeft".into(), "4".into()],
        };
        profile.rules.push(rule);
        settings.profiles = vec![profile];

        let rules = CompiledRules::compile(&settings).unwrap();
        let mut engine = RuntimeEngine::new(rules);
        for key in ["MetaLeft", "ShiftLeft"] {
            let dispatch = engine.process(
                &KeyEvent {
                    key: key.into(),
                    phase: KeyPhase::Down,
                    origin: EventOrigin::Physical,
                    repeat: false,
                },
                &MatchContext::default(),
            );
            assert!(dispatch.actions.is_empty());
        }

        let dispatch = engine.process(
            &KeyEvent {
                key: "S".into(),
                phase: KeyPhase::Down,
                origin: EventOrigin::Physical,
                repeat: false,
            },
            &MatchContext::default(),
        );

        assert!(dispatch.suppress_original);
        assert_eq!(dispatch.actions.len(), 1);
        assert_eq!(dispatch.actions[0].phase, DispatchActionPhase::Invoke);
        assert_eq!(
            dispatch.actions[0].transient_release_inputs,
            vec!["metaleft"]
        );
        assert!(matches!(
            &dispatch.actions[0].action,
            Action::SendKeys { chord }
                if chord == &vec!["ControlLeft", "ShiftLeft", "4"]
        ));
    }

    #[test]
    fn remapped_modifier_shortcuts_release_the_active_output_not_the_physical_key() {
        let mut settings = Settings::default();
        let mut profile = Profile::new("remapped modifier shortcut");
        profile
            .rules
            .push(Rule::key_remap("ControlLeft", "AltLeft"));
        let mut rule = Rule::key_remap("K", "F24");
        rule.trigger = Trigger::Keyboard {
            chord: vec!["ControlLeft".into(), "K".into()],
            phase: TriggerPhase::Press,
            gesture: TriggerGesture::Single,
        };
        profile.rules.push(rule);
        settings.profiles = vec![profile];

        let rules = CompiledRules::compile(&settings).unwrap();
        let mut engine = RuntimeEngine::new(rules);

        let modifier_down = engine.process(
            &KeyEvent {
                key: "ControlLeft".into(),
                phase: KeyPhase::Down,
                origin: EventOrigin::Physical,
                repeat: false,
            },
            &MatchContext::default(),
        );
        assert!(matches!(
            &modifier_down.actions[0].action,
            Action::SendKeys { chord } if chord == &vec!["AltLeft"]
        ));

        let dispatch = engine.process(
            &KeyEvent {
                key: "K".into(),
                phase: KeyPhase::Down,
                origin: EventOrigin::Physical,
                repeat: false,
            },
            &MatchContext::default(),
        );

        assert!(dispatch.suppress_original);
        assert_eq!(dispatch.actions.len(), 1);
        assert_eq!(dispatch.actions[0].phase, DispatchActionPhase::Invoke);
        assert_eq!(
            dispatch.actions[0].transient_release_inputs,
            vec!["altleft"]
        );
        assert!(matches!(
            &dispatch.actions[0].action,
            Action::SendKeys { chord } if chord == &vec!["F24"]
        ));
    }
    #[test]
    fn detects_duplicate_global_triggers() {
        let mut settings = settings_with_remap();
        let mut second = Profile::new("other");
        second.rules.push(Rule::key_remap("A", "C"));
        settings.profiles.push(second);
        assert!(matches!(
            CompiledRules::compile(&settings),
            Err(CompileError::Conflict { .. })
        ));
    }

    #[test]
    fn pass_through_rule_does_not_consume_key_up() {
        let mut settings = settings_with_remap();
        settings.profiles[0].rules[0].options.pass_through_original = true;
        let rules = CompiledRules::compile(&settings).unwrap();
        let mut engine = RuntimeEngine::new(rules);
        let down = engine.process(
            &KeyEvent {
                key: "A".into(),
                phase: KeyPhase::Down,
                origin: EventOrigin::Physical,
                repeat: false,
            },
            &MatchContext::default(),
        );
        let up = engine.process(
            &KeyEvent {
                key: "A".into(),
                phase: KeyPhase::Up,
                origin: EventOrigin::Physical,
                repeat: false,
            },
            &MatchContext::default(),
        );
        assert!(!down.suppress_original);
        assert!(!up.suppress_original);
    }

    #[test]
    fn failed_injection_can_cancel_key_up_consumption() {
        let rules = CompiledRules::compile(&settings_with_remap()).unwrap();
        let mut engine = RuntimeEngine::new(rules);
        engine.process(
            &KeyEvent {
                key: "A".into(),
                phase: KeyPhase::Down,
                origin: EventOrigin::Physical,
                repeat: false,
            },
            &MatchContext::default(),
        );
        engine.cancel_consumed("A");
        let up = engine.process(
            &KeyEvent {
                key: "A".into(),
                phase: KeyPhase::Up,
                origin: EventOrigin::Physical,
                repeat: false,
            },
            &MatchContext::default(),
        );
        assert!(!up.suppress_original);
    }

    #[test]
    fn resume_after_emergency_stop_processes_rules_again() {
        let rules = CompiledRules::compile(&settings_with_remap()).unwrap();
        let mut engine = RuntimeEngine::new(rules);
        for key in ["Control", "Alt", "Pause"] {
            engine.process(
                &KeyEvent {
                    key: key.into(),
                    phase: KeyPhase::Down,
                    origin: EventOrigin::Physical,
                    repeat: false,
                },
                &MatchContext::default(),
            );
        }
        for key in ["Pause", "Alt", "Control"] {
            engine.process(
                &KeyEvent {
                    key: key.into(),
                    phase: KeyPhase::Up,
                    origin: EventOrigin::Physical,
                    repeat: false,
                },
                &MatchContext::default(),
            );
        }
        engine.set_paused(false);
        let dispatch = engine.process(
            &KeyEvent {
                key: "A".into(),
                phase: KeyPhase::Down,
                origin: EventOrigin::Physical,
                repeat: false,
            },
            &MatchContext::default(),
        );
        assert!(dispatch.suppress_original);
    }
}
