use crate::diff;
use crate::types::{DaemonResponse, daemon_response};

use super::execution::render_execution_result;
use super::formatters::Formatter;
use super::helpers::format_duration;
use super::inspect::render_class_fingerprints;
use super::style;

#[derive(Default)]
pub(crate) struct RenderOutput {
    pub(crate) stdout: String,
    pub(crate) stderr: String,
}

impl RenderOutput {
    pub(super) fn push_stdout(&mut self, text: impl AsRef<str>) {
        self.stdout.push_str(text.as_ref());
    }

    pub(super) fn push_stderr(&mut self, text: impl AsRef<str>) {
        self.stderr.push_str(text.as_ref());
    }

    pub(super) fn extend(&mut self, other: Self) {
        let Self { stdout, stderr } = other;
        self.stdout.push_str(&stdout);
        self.stderr.push_str(&stderr);
    }
}

pub(super) fn stdout_only(text: String) -> RenderOutput {
    RenderOutput {
        stdout: text,
        stderr: String::new(),
    }
}

pub(super) fn stderr_only(text: String) -> RenderOutput {
    RenderOutput {
        stdout: String::new(),
        stderr: text,
    }
}

pub(super) fn render_with(response: &DaemonResponse, fmt: &dyn Formatter) -> RenderOutput {
    match response.kind_validated() {
        daemon_response::Kind::ApkLoaded(payload) => render_apk_loaded(payload, fmt),
        daemon_response::Kind::ApkUnloaded(_) => stdout_only(fmt.success("APK unloaded.")),
        daemon_response::Kind::StatusInfo(payload) => render_status_info(payload, fmt),
        daemon_response::Kind::ExecutionResult(payload) => {
            let mut output = RenderOutput::default();
            if matches!(fmt.render_mode(), diff::RenderMode::Markdown) {
                output.push_stdout(fmt.heading(1, "Execution results"));
            }
            output.extend(render_execution_result(&payload.events, fmt));
            output
        }
        daemon_response::Kind::FingerprintResult(payload) => {
            render_fingerprint_result(payload, fmt)
        }
        daemon_response::Kind::ClassFingerprintResult(payload) => {
            stdout_only(render_class_fingerprints(payload, fmt))
        }
        daemon_response::Kind::CommonFingerprintResult(payload) => {
            render_common_fingerprint_result(payload, fmt)
        }
        daemon_response::Kind::SearchResult(payload) => render_search_result(payload, fmt),
        daemon_response::Kind::MethodMap(payload) => render_method_map(payload, fmt),
        daemon_response::Kind::MethodSmali(payload) => render_method_smali(payload, fmt),
        daemon_response::Kind::Ok(_) => stdout_only(fmt.success("OK")),
        daemon_response::Kind::Error(payload) => stderr_only(fmt.error(&payload.message)),
    }
}

fn render_apk_loaded(
    payload: &crate::types::ApkLoadedResponse,
    fmt: &dyn Formatter,
) -> RenderOutput {
    let mut output = RenderOutput::default();

    if let Some(identity) = payload.identity.as_ref() {
        if matches!(fmt.render_mode(), diff::RenderMode::Markdown) {
            output.push_stdout(fmt.heading(2, "APK loaded"));
            output.push_stdout(fmt.bullet(&format!(
                "{}: {}",
                fmt.bold("Package"),
                identity.package_name
            )));
            output.push_stdout(fmt.bullet(&format!(
                "{}: {}",
                fmt.bold("Version"),
                identity.package_version
            )));
            output.push_stdout(fmt.bullet(&format!(
                "{}: {}",
                fmt.bold("Path"),
                identity.source_file_path
            )));
        } else {
            output.push_stdout(format!(
                "{} {} {}\n",
                style::success("APK loaded:"),
                identity.package_name,
                identity.package_version,
            ));
        }
    } else {
        output.push_stdout(fmt.warning("APK already loaded or invalid."));
    }

    output
}

fn render_status_info(
    payload: &crate::types::StatusInfoResponse,
    fmt: &dyn Formatter,
) -> RenderOutput {
    let mut output = RenderOutput::default();

    if matches!(fmt.render_mode(), diff::RenderMode::Markdown) {
        output.push_stdout(fmt.heading(2, "Daemon status"));
        output.push_stdout(fmt.bullet(&format!(
            "{}: {}",
            fmt.bold("Uptime"),
            format_duration(payload.uptime_secs)
        )));
        output.push_stdout(fmt.bullet(&format!(
            "{}: {}\n",
            fmt.bold("Loaded APKs"),
            payload.apks.len()
        )));
        for apk in &payload.apks {
            let identity = apk.identity_ref();
            output.push_stdout(format!(
                "  - {} {}\n    - Classes: {}\n    - Methods: {}\n    - Path: {}\n",
                identity.package_name,
                identity.package_version,
                apk.class_count,
                apk.method_count,
                identity.source_file_path,
            ));
        }
    } else {
        output.push_stdout(format!(
            "{} — uptime: {}, {} APK(s) loaded\n",
            style::bold("Daemon status"),
            format_duration(payload.uptime_secs),
            payload.apks.len(),
        ));
        for apk in &payload.apks {
            let identity = apk.identity_ref();
            output.push_stdout(format!(
                "  {} {} ({}, {})\n",
                identity.package_name,
                identity.package_version,
                style::cyan(&format!("{} classes", apk.class_count)),
                style::green(&format!("{} methods", apk.method_count)),
            ));
        }
    }

    output
}

fn render_fingerprint_result(
    payload: &crate::types::FingerprintResultResponse,
    fmt: &dyn Formatter,
) -> RenderOutput {
    let mut output = RenderOutput::default();

    if matches!(fmt.render_mode(), diff::RenderMode::Markdown) {
        output.push_stdout(fmt.heading(2, "Fingerprints"));
        output.push_stdout(format!(
            "Generated {} fingerprint(s). Use the results below as-is; they already target unique matches.\n\n",
            payload.fingerprints.len()
        ));
        for (index, fingerprint) in payload.fingerprints.iter().enumerate() {
            output.push_stdout(fmt.heading(3, &format!("Fingerprint {}", index + 1)));
            output.push_stdout(fmt.code_block("kotlin", &fingerprint.morphe_code));
        }
    } else {
        output.push_stdout(format!(
            "{}\n\n",
            style::bold(&format!(
                "{} fingerprint(s). Use the results below as-is; they already target unique matches",
                payload.fingerprints.len()
            ))
        ));
        for (index, fingerprint) in payload.fingerprints.iter().enumerate() {
            output.push_stdout(format!("{}\n", style::cyan(&format!("#{}", index + 1))));
            output.push_stdout(fmt.code_block("", &fingerprint.morphe_code));
        }
    }

    output
}

fn render_common_fingerprint_result(
    payload: &crate::types::CommonFingerprintResultResponse,
    fmt: &dyn Formatter,
) -> RenderOutput {
    let mut output = RenderOutput::default();
    let is_md = matches!(fmt.render_mode(), diff::RenderMode::Markdown);

    if is_md {
        output.push_stdout(fmt.heading(2, "Common Fingerprints"));
        output.push_stdout("Targets:\n");
        for target in &payload.targets {
            let apk = target.apk_ref();
            let method = target.method_ref();
            output.push_stdout(fmt.bullet(&format!(
                "{}: {}",
                fmt.code(&apk_identity_label(apk)),
                fmt.smali_method_id(method)
            )));
        }
        output.push_stdout(format!(
            "\nGenerated {} common fingerprint(s). Each result uniquely targets the selected method in every APK.\n\n",
            payload.fingerprints.len()
        ));
        for (index, fingerprint) in payload.fingerprints.iter().enumerate() {
            output.push_stdout(fmt.heading(3, &format!("Fingerprint {}", index + 1)));
            output.push_stdout(fmt.code_block("kotlin", &fingerprint.morphe_code));
        }
    } else {
        output.push_stdout(format!("{}\n", style::bold("Common fingerprints")));
        for target in &payload.targets {
            let apk = target.apk_ref();
            let method = target.method_ref();
            output.push_stdout(format!(
                "  {} {} -> {}\n",
                style::dimmed("target:"),
                apk_identity_label(apk),
                fmt.smali_method_id(method)
            ));
        }
        output.push_stdout(format!(
            "\n{}\n\n",
            style::bold(&format!(
                "{} common fingerprint(s). Each result uniquely targets the selected method in every APK",
                payload.fingerprints.len()
            ))
        ));
        for (index, fingerprint) in payload.fingerprints.iter().enumerate() {
            output.push_stdout(format!("{}\n", style::cyan(&format!("#{}", index + 1))));
            output.push_stdout(fmt.code_block("", &fingerprint.morphe_code));
        }
    }

    output
}

fn render_search_result(
    payload: &crate::types::SearchResultResponse,
    fmt: &dyn Formatter,
) -> RenderOutput {
    let mut output = RenderOutput::default();
    let is_md = matches!(fmt.render_mode(), diff::RenderMode::Markdown);

    if is_md {
        output.push_stdout(fmt.heading(2, "Search results"));
    }
    for (apk_label, methods) in &payload.results {
        if is_md {
            output.push_stdout(fmt.heading(3, &format!("APK {apk_label}")));
            for method in &methods.items {
                output.push_stdout(fmt.bullet(&fmt.smali_method_id(method)));
            }
            output.push_stdout("\n");
        } else {
            output.push_stdout(format!("{}\n", style::bold(&format!("APK {apk_label}"))));
            for method in &methods.items {
                output.push_stdout(format!("  {}\n", fmt.smali_method_id(method)));
            }
        }
    }

    output
}

fn render_method_map(
    payload: &crate::types::MethodMapResponse,
    fmt: &dyn Formatter,
) -> RenderOutput {
    let mut output = RenderOutput::default();
    let is_md = matches!(fmt.render_mode(), diff::RenderMode::Markdown);
    let source_apk = payload.source_apk_ref();
    let target_apk = payload.target_apk_ref();
    let source_method = payload.source_method_ref();

    if is_md {
        output.push_stdout(fmt.heading(2, "Method map"));
        output.push_stdout(fmt.bullet(&format!(
            "{}: {}",
            fmt.bold("Source APK"),
            fmt.code(&apk_identity_label(source_apk))
        )));
        output.push_stdout(fmt.bullet(&format!(
            "{}: {}",
            fmt.bold("Source method"),
            fmt.smali_method_id(source_method)
        )));
        output.push_stdout(fmt.bullet(&format!(
            "{}: {}\n",
            fmt.bold("Target APK"),
            fmt.code(&apk_identity_label(target_apk))
        )));
        output.push_stdout(format!(
            "Found {} similar method(s), ranked from most to least similar.\n\n",
            payload.candidates.len()
        ));
        output.push_stdout("| # | Similarity | Method |\n|--:|-----------:|--------|\n");
        for (index, candidate) in payload.candidates.iter().enumerate() {
            if let Some(method) = candidate.method.as_ref() {
                output.push_stdout(format!(
                    "| {} | {:.1}% | {} |\n",
                    index + 1,
                    candidate.similarity,
                    fmt.smali_method_id(method)
                ));
            }
        }
        output.push_stdout("\n");
    } else {
        output.push_stdout(format!("{}\n", style::bold("Method map")));
        output.push_stdout(format!(
            "  {} {}\n",
            style::dimmed("source apk:"),
            apk_identity_label(source_apk)
        ));
        output.push_stdout(format!(
            "  {} {}\n",
            style::dimmed("source method:"),
            fmt.smali_method_id(source_method)
        ));
        output.push_stdout(format!(
            "  {} {}\n\n",
            style::dimmed("target apk:"),
            apk_identity_label(target_apk)
        ));
        for (index, candidate) in payload.candidates.iter().enumerate() {
            if let Some(method) = candidate.method.as_ref() {
                output.push_stdout(format!(
                    "  {:>3}. [{:>6.1}%] {}\n",
                    index + 1,
                    candidate.similarity,
                    fmt.smali_method_id(method)
                ));
            }
        }
    }

    output
}

fn apk_identity_label(identity: &crate::types::ApkIdentity) -> String {
    format!("{} / {}", identity.package_name, identity.package_version)
}

fn render_method_smali(
    payload: &crate::types::MethodSmaliResponse,
    fmt: &dyn Formatter,
) -> RenderOutput {
    let Some(smali) = payload.smali.as_ref() else {
        return stdout_only(fmt.warning("Method not found."));
    };

    if matches!(fmt.render_mode(), diff::RenderMode::Markdown) {
        stdout_only(fmt.code_block("smali", smali))
    } else {
        stdout_only(format!("{smali}\n"))
    }
}

