use crate::types::{ApkIdentity, MethodInfoDto};
use nucleo_matcher::pattern::{AtomKind, CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32String};
use std::collections::HashMap;

/// Build a searchable text for a method from its info fields.
/// Combines class name, method name, descriptor, return type, access flags, java forms.
fn build_search_haystack(info: &MethodInfoDto) -> String {
    let mut text = String::with_capacity(256);

    // Class name (both smali and java forms)
    text.push_str(&info.defining_class);
    text.push(' ');
    text.push_str(&info.class_name);
    text.push(' ');

    // Method name
    text.push_str(&info.name);
    text.push(' ');

    // Unique id and short id
    text.push_str(&info.unique_id);
    text.push(' ');
    text.push_str(&info.short_id);
    text.push(' ');

    // Java signature
    text.push_str(&info.java_signature);
    text.push(' ');

    // Return type (both forms)
    text.push_str(&info.return_type);
    text.push(' ');
    text.push_str(&info.java_return_type);
    text.push(' ');

    // Parameter types
    for p in &info.parameters {
        text.push_str(p);
        text.push(' ');
    }
    for p in &info.java_parameter_types {
        text.push_str(p);
        text.push(' ');
    }

    // Access flags
    for f in &info.java_access_flags {
        text.push_str(f);
        text.push(' ');
    }

    text
}

/// Build cached haystacks for all methods in an APK.
pub fn build_search_index(methods: &[MethodInfoDto]) -> Vec<Utf32String> {
    methods
        .iter()
        .map(|method| Utf32String::from(build_search_haystack(method)))
        .collect()
}

/// Search across all loaded APKs using nucleo fuzzy matching.
/// Query words are matched independently and their scores are summed.
/// Returns a map of APK display label → matching `MethodInfoDtos`, sorted by match score.
pub fn search_all_apks<'a>(
    apks: impl Iterator<Item = (&'a ApkIdentity, &'a [MethodInfoDto], &'a [Utf32String])>,
    query: &str,
    limit: usize,
) -> HashMap<String, Vec<MethodInfoDto>> {
    let mut matcher = Matcher::new(Config::DEFAULT.match_paths());
    let pattern = Pattern::new(
        query,
        CaseMatching::Ignore,
        Normalization::Smart,
        AtomKind::Fuzzy,
    );

    let mut results: HashMap<String, Vec<MethodInfoDto>> = HashMap::new();

    for (identity, methods, haystacks) in apks {
        debug_assert_eq!(methods.len(), haystacks.len());
        let mut scored: Vec<(u32, MethodInfoDto)> = Vec::new();

        for (method, haystack) in methods.iter().zip(haystacks.iter()) {
            if let Some(score) = pattern.score(haystack.slice(..), &mut matcher) {
                scored.push((score, method.clone()));
            }
        }

        scored.sort_by_key(|score| std::cmp::Reverse(score.0));
        scored.truncate(limit);

        if !scored.is_empty() {
            let infos: Vec<MethodInfoDto> = scored.into_iter().map(|(_, info)| info).collect();
            results.insert(
                format!("{} / {}", identity.package_name, identity.package_version),
                infos,
            );
        }
    }

    results
}
