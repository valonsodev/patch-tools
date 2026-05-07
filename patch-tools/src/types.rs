use anyhow::{Context, Result};

mod generated {
    #![allow(clippy::derive_partial_eq_without_eq)]
    #![allow(clippy::doc_markdown)]
    #![allow(clippy::trivially_copy_pass_by_ref)]

    include!(concat!(env!("OUT_DIR"), "/patch_tools.rs"));
}

pub use generated::*;

impl DaemonRequest {
    pub fn load_apk(path: impl Into<String>) -> Self {
        Self {
            kind: Some(daemon_request::Kind::LoadApk(LoadApkRequest {
                path: path.into(),
            })),
        }
    }

    pub fn unload_apk(apk_selector: impl Into<String>) -> Self {
        Self {
            kind: Some(daemon_request::Kind::UnloadApk(UnloadApkRequest {
                apk_id: apk_selector.into(),
            })),
        }
    }

    pub fn execute(
        script_path: impl Into<String>,
        fingerprint_result_cap: Option<u32>,
        save_patched_apks: bool,
    ) -> Self {
        Self {
            kind: Some(daemon_request::Kind::Execute(ExecuteRequest {
                script_path: script_path.into(),
                fingerprint_result_cap,
                save_patched_apks,
            })),
        }
    }

    pub fn generate_fingerprint(
        apk_selector: impl Into<String>,
        method_id: impl Into<String>,
        limit: Option<u32>,
    ) -> Self {
        Self {
            kind: Some(daemon_request::Kind::GenerateFingerprint(
                GenerateFingerprintRequest {
                    apk_id: apk_selector.into(),
                    method_id: method_id.into(),
                    limit,
                },
            )),
        }
    }

    pub fn generate_class_fingerprint(
        apk_selector: impl Into<String>,
        class_id: impl Into<String>,
        limit: Option<u32>,
    ) -> Self {
        Self {
            kind: Some(daemon_request::Kind::GenerateClassFingerprint(
                GenerateClassFingerprintRequest {
                    apk_id: apk_selector.into(),
                    class_id: class_id.into(),
                    limit,
                },
            )),
        }
    }

    pub fn search_methods(query: impl Into<String>, limit: Option<u32>) -> Self {
        Self {
            kind: Some(daemon_request::Kind::SearchMethods(SearchMethodsRequest {
                query: query.into(),
                limit,
            })),
        }
    }

    pub fn map_method(
        old_apk_selector: impl Into<String>,
        method_id: impl Into<String>,
        new_apk_selector: impl Into<String>,
        limit: Option<u32>,
    ) -> Self {
        Self {
            kind: Some(daemon_request::Kind::MapMethod(MapMethodRequest {
                old_apk_id: old_apk_selector.into(),
                method_id: method_id.into(),
                new_apk_id: new_apk_selector.into(),
                limit,
            })),
        }
    }

    pub fn get_method_smali(apk_selector: impl Into<String>, method_id: impl Into<String>) -> Self {
        Self {
            kind: Some(daemon_request::Kind::GetMethodSmali(
                GetMethodSmaliRequest {
                    apk_id: apk_selector.into(),
                    method_id: method_id.into(),
                },
            )),
        }
    }

    pub fn status() -> Self {
        Self {
            kind: Some(daemon_request::Kind::Status(StatusRequest {})),
        }
    }

    pub fn stop() -> Self {
        Self {
            kind: Some(daemon_request::Kind::Stop(StopRequest {})),
        }
    }

    pub fn kind_ref(&self) -> Result<&daemon_request::Kind> {
        self.kind.as_ref().context("daemon request kind missing")
    }

    pub fn into_kind(self) -> Result<daemon_request::Kind> {
        self.kind.context("daemon request kind missing")
    }
}

impl DaemonResponse {
    pub fn ok() -> Self {
        Self {
            kind: Some(daemon_response::Kind::Ok(OkResponse {})),
        }
    }

    pub fn error(message: impl Into<String>) -> Self {
        Self {
            kind: Some(daemon_response::Kind::Error(ErrorResponse {
                message: message.into(),
            })),
        }
    }

    pub fn kind_ref(&self) -> Result<&daemon_response::Kind> {
        self.kind.as_ref().context("daemon response kind missing")
    }
}

/// Generate a `fn $method(&self) -> &$ret` accessor that unwraps a required protobuf field.
macro_rules! required_ref {
    ($type:ty, $method:ident -> $ret:ty, $field:ident, $msg:expr) => {
        impl $type {
            pub fn $method(&self) -> &$ret {
                self.$field.as_ref().expect($msg)
            }
        }
    };
}

required_ref!(MethodData, info_ref -> MethodInfoDto, info, "validated method data missing info");
required_ref!(InstructionFeature, kind_ref -> instruction_feature::Kind, kind, "validated instruction feature missing kind");
required_ref!(EngineEvent, kind_ref -> engine_event::Kind, kind, "validated engine event missing kind");
required_ref!(EngineResult, kind_ref -> engine_result::Kind, kind, "validated engine result missing kind");

impl MethodDiffDto {
    pub fn change_kind_enum(&self) -> method_diff_dto::ChangeKind {
        method_diff_dto::ChangeKind::try_from(self.change_kind)
            .unwrap_or(method_diff_dto::ChangeKind::Unspecified)
    }
}

impl ClassDiffDto {
    pub fn change_kind_enum(&self) -> class_diff_dto::ChangeKind {
        class_diff_dto::ChangeKind::try_from(self.change_kind)
            .unwrap_or(class_diff_dto::ChangeKind::Unspecified)
    }
}

impl ResourceChangeDto {
    pub fn kind_enum(&self) -> resource_change_dto::Kind {
        resource_change_dto::Kind::try_from(self.kind)
            .unwrap_or(resource_change_dto::Kind::Unspecified)
    }
}

impl MethodFingerprintDto {
    pub fn parameter_values(&self) -> Option<&[String]> {
        self.parameters
            .as_ref()
            .map(|parameters| parameters.values.as_slice())
    }
}
