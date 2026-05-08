//! APK / method / class selector resolution. Pure functions over a borrow of
//! the daemon's [`SharedState`] (as defined by `daemon::server`); the resolver
//! itself is stateless.

use super::engine_worker::ApkData;
use crate::types::ApkIdentity;
use std::collections::HashMap;
use std::sync::Arc;

/// State shape required by the resolver. Defined as a trait-free struct view so
/// the resolver doesn't need to depend on the server's full state struct.
pub struct StateView<'a> {
    pub apks: &'a HashMap<String, Arc<ApkData>>,
}

pub fn resolve_apk_selector(
    state: &StateView<'_>,
    raw: Option<&str>,
) -> std::result::Result<String, String> {
    let Some(raw) = raw else {
        return resolve_implicit_apk_selector(state);
    };
    let normalized = raw.trim();
    if normalized.is_empty() {
        return resolve_implicit_apk_selector(state);
    }

    if state.apks.contains_key(normalized) {
        return Ok(normalized.to_string());
    }

    let mut exact_matches = Vec::new();
    let mut package_matches = Vec::new();

    for (apk_id, apk) in state.apks {
        let identity = &apk.identity;
        let package = identity.package_name.as_str();
        let version = identity.package_version.as_str();
        let selectors = [
            format!("{package} / {version}"),
            format!("{package}/{version}"),
            format!("{package}@{version}"),
            format!("{package}:{version}"),
            format!("{package} {version}"),
        ];

        if selectors.iter().any(|selector| selector == normalized) {
            exact_matches.push((apk_id.clone(), apk_display_name(identity)));
        } else if package == normalized {
            package_matches.push((apk_id.clone(), apk_display_name(identity)));
        }
    }

    match exact_matches.len() {
        1 => return Ok(exact_matches.remove(0).0),
        n if n > 1 => {
            return Err(format!(
                "APK selector '{}' is ambiguous. Matches: {}",
                normalized,
                exact_matches
                    .into_iter()
                    .map(|(_, label)| label)
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        _ => {}
    }

    match package_matches.len() {
        1 => Ok(package_matches.remove(0).0),
        n if n > 1 => Err(format!(
            "APK selector '{}' matches multiple versions. Use 'package / version'. Matches: {}",
            normalized,
            package_matches
                .into_iter()
                .map(|(_, label)| label)
                .collect::<Vec<_>>()
                .join(", ")
        )),
        _ => Err(format_apk_not_found_error(state, normalized)),
    }
}

fn resolve_implicit_apk_selector(state: &StateView<'_>) -> std::result::Result<String, String> {
    match state.apks.len() {
        0 => Err("APK selector omitted, but no APKs are loaded".to_string()),
        1 => Ok(state
            .apks
            .keys()
            .next()
            .expect("one APK exists")
            .clone()),
        _ => Err(format!(
            "APK selector is required when multiple APKs are loaded. Available apks: {}",
            available_apk_labels(state)
        )),
    }
}

pub fn format_apk_not_found_error(state: &StateView<'_>, raw: &str) -> String {
    format!(
        "APK not found: {raw}. Available apks: {}",
        available_apk_labels(state)
    )
}

fn available_apk_labels(state: &StateView<'_>) -> String {
    let mut labels = state
        .apks
        .values()
        .map(|apk| apk_display_name(&apk.identity))
        .collect::<Vec<_>>();
    labels.sort();

    if labels.is_empty() {
        "none loaded".to_string()
    } else {
        labels.join(", ")
    }
}

pub fn apk_display_name(identity: &ApkIdentity) -> String {
    format!("{} / {}", identity.package_name, identity.package_version)
}

/// Resolve a `method_id` which may be a `unique_id` or `java_signature`.
pub fn resolve_method_id(apk: &ApkData, raw: &str) -> Option<String> {
    if apk
        .method_infos
        .iter()
        .any(|method| method.unique_id == raw)
    {
        return Some(raw.to_string());
    }

    apk.method_infos
        .iter()
        .find(|method| method.java_signature == raw)
        .map(|method| method.unique_id.clone())
}

pub fn resolve_class_id(apk: &ApkData, raw: &str) -> std::result::Result<String, String> {
    let normalized = raw.trim();
    if normalized.is_empty() {
        return Err("Class selector cannot be empty".to_string());
    }

    let mut matches = Vec::new();
    for method in &apk.method_infos {
        if method.defining_class == normalized || method.class_name == normalized {
            matches.push(method.defining_class.clone());
        }
    }

    matches.sort();
    matches.dedup();

    match matches.len() {
        1 => Ok(matches.remove(0)),
        0 => Err(format!("Class not found: {normalized}")),
        _ => Err(format!(
            "Class selector '{}' is ambiguous. Matches: {}",
            normalized,
            matches.join(", ")
        )),
    }
}
