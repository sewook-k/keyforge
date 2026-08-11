mod action_result;
mod model;
mod repository;
mod validation;

pub use action_result::*;
pub use model::*;
pub use repository::*;
pub use validation::*;

#[cfg(test)]
mod contract_tests {
    use super::*;

    #[test]
    fn supported_action_uses_the_stable_wire_format() {
        let action = Action::SendMouse {
            button: MouseButton::X1,
        };
        let value = serde_json::to_value(&action).unwrap();
        assert_eq!(value["kind"], "send_mouse");
        assert_eq!(value["button"], "x1");
    }

    #[test]
    fn empty_emergency_chord_is_rejected() {
        let mut settings = Settings::default();
        settings.engine.emergency_stop.clear();
        assert_eq!(
            validate(&settings),
            Err(ValidationError::InvalidEmergencyStop)
        );
    }

    #[test]
    fn keyboard_activation_requires_a_stable_identifier() {
        let mut settings = Settings::default();
        let profile = settings.profiles.first_mut().unwrap();
        let profile_id = profile.id;
        profile
            .activation
            .connected_keyboards
            .push(ConnectedKeyboardActivation {
                vendor_id: None,
                product_id: None,
                interface_id: None,
                manufacturer_contains: None,
                name_contains: None,
                is_virtual: false,
            });

        assert_eq!(
            validate(&settings),
            Err(ValidationError::UnsafeKeyboardActivation(profile_id))
        );
    }
}
