// Shared by the Compose UI and SettingsStore so transient and saved selections
// enforce the same bounds.
pub(super) const MAX_COMPOSE_FILES: usize = 16;
pub(super) const MAX_COMPOSE_REGISTRATIONS: usize = 32;
pub(super) const MAX_COMPOSE_REGISTRATION_LABEL_BYTES: usize = 256;
pub(super) const MAX_COMPOSE_PROFILES: usize = 16;
pub(super) const MAX_COMPOSE_PROFILE_NAME_BYTES: usize = 64;
pub(super) const MAX_COMPOSE_PROFILE_INPUT_BYTES: usize = 1024;

fn valid_compose_profile_name(name: &str) -> bool {
    name.len() >= 2
        && name.len() <= MAX_COMPOSE_PROFILE_NAME_BYTES
        && name.as_bytes().iter().enumerate().all(|(index, byte)| {
            if index == 0 {
                byte.is_ascii_alphanumeric()
            } else {
                byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-')
            }
        })
}

pub(super) fn parse_compose_profiles(input: &str) -> Result<Vec<String>, ()> {
    if input.len() > MAX_COMPOSE_PROFILE_INPUT_BYTES {
        return Err(());
    }
    if input.trim().is_empty() {
        return Ok(Vec::new());
    }
    let mut profiles = Vec::new();
    for token in input.split(',') {
        let name = token.trim();
        if name.is_empty() || (name != "*" && !valid_compose_profile_name(name)) {
            return Err(());
        }
        if name == "*" && (!profiles.is_empty() || input.split(',').count() != 1) {
            return Err(());
        }
        if profiles.iter().any(|profile| profile == name) {
            return Err(());
        }
        profiles.push(name.to_owned());
        if profiles.len() > MAX_COMPOSE_PROFILES {
            return Err(());
        }
    }
    Ok(profiles)
}

pub(super) fn valid_profile_list(profiles: &[String]) -> bool {
    if profiles.len() > MAX_COMPOSE_PROFILES {
        return false;
    }
    let input_bytes = profiles
        .iter()
        .fold(0_usize, |total, profile| {
            total.saturating_add(profile.len())
        })
        .saturating_add(profiles.len().saturating_sub(1));
    input_bytes <= MAX_COMPOSE_PROFILE_INPUT_BYTES
        && profiles.iter().enumerate().all(|(index, name)| {
            (name == "*" && profiles.len() == 1 || valid_compose_profile_name(name))
                && !profiles[..index].iter().any(|previous| previous == name)
        })
}
