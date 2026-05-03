use crate::types::{
    ApkIdentity, ApkStatus, EngineEvent, ExecutionResultResponse, MethodData, MethodDataList,
    engine_event,
};
use anyhow::{Context, Result};
use jni::objects::{Global, JByteArray, JObject, JString, JValue};
use jni::strings::JNIString;
use jni::{InitArgsBuilder, JavaVM, jni_sig, jni_str};
use prost::Message;
use std::io::Write;
use tempfile::{Builder, NamedTempFile};

const ENGINE_JAR: &[u8] = include_bytes!(env!("MORPHE_ENGINE_JAR"));

/// Rust wrapper around the Kotlin `JniFacade`.
pub struct EngineJni {
    jvm: JavaVM,
    facade: Global<JObject<'static>>,
    _engine_jar: NamedTempFile,
}

impl EngineJni {
    pub fn new() -> Result<Self> {
        let mut engine_jar = Builder::new()
            .prefix("morphe-engine-")
            .suffix(".jar")
            .tempfile()
            .context("failed to create temp engine JAR")?;
        engine_jar
            .as_file_mut()
            .write_all(ENGINE_JAR)
            .context("failed to write embedded engine JAR")?;
        engine_jar
            .as_file_mut()
            .flush()
            .context("failed to flush embedded engine JAR")?;

        let jar = engine_jar.path().display().to_string();
        let classpath = format!("-Djava.class.path={jar}");

        // KotlinJars.getLib() scans for individual JARs by name or via
        // Thread.contextClassLoader (null on JNI-attached threads → NPE).
        // Setting these properties bypasses the scan entirely.
        let props: Vec<String> = [
            "kotlin.java.stdlib.jar",
            "kotlin.java.runtime.jar",
            "kotlin.java.reflect.jar",
            "kotlin.script.runtime.jar",
            "kotlin.compiler.classpath",
            "kotlin.script.classpath",
        ]
        .iter()
        .map(|key| format!("-D{key}={jar}"))
        .collect();

        let mut builder = InitArgsBuilder::new()
            .version(jni::JNIVersion::V1_8)
            .option(&classpath)
            .option("-Xmx2g");
        for prop in &props {
            builder = builder.option(prop);
        }
        let jvm_args = builder.build().context("failed to build JVM args")?;

        let jvm = JavaVM::new(jvm_args).context("failed to create JVM")?;

        let facade = {
            jvm.attach_current_thread(|env| -> Result<_> {
                let cls = env
                    .find_class(jni_str!("dev/valonso/tools/engine/jni/JniFacade"))
                    .context("JniFacade class not found")?;
                let local = env
                    .new_object(&cls, jni_sig!("()V"), &[])
                    .context("failed to create JniFacade")?;
                env.new_global_ref(local)
                    .context("failed to create global ref")
            })
            .context("failed to attach thread")?
        };

        Ok(Self {
            jvm,
            facade,
            _engine_jar: engine_jar,
        })
    }

    // -- APK lifecycle --------------------------------------------------------

    pub fn load_apk(&self, path: &str) -> Result<Option<ApkIdentity>> {
        match self.call_bytes_method_1("loadApkProto", path)? {
            Some(bytes) => Ok(Some(decode_message(
                bytes.as_slice(),
                "deserialize ApkIdentity",
            )?)),
            None => Ok(None),
        }
    }

    pub fn unload_apk(&self, apk_id: &str) -> Result<()> {
        self.jvm
            .attach_current_thread(|env| -> Result<()> {
                let arg = env.new_string(apk_id).context("new_string")?;
                env.call_method(
                    &self.facade,
                    jni_str!("unloadApk"),
                    jni_sig!("(Ljava/lang/String;)V"),
                    &[JValue::Object(&arg)],
                )
                .context("unloadApk call failed")?;
                Ok(())
            })
            .context("attach")
    }

    pub fn get_apk_status(&self, apk_id: &str) -> Result<ApkStatus> {
        let bytes = self
            .call_bytes_method_1("getApkStatusProto", apk_id)?
            .context("getApkStatusProto returned null")?;
        decode_message(bytes.as_slice(), "deserialize apk status")
    }

    // -- Method info ----------------------------------------------------------

    pub fn get_method_smali(&self, apk_id: &str, method_id: &str) -> Result<Option<String>> {
        self.jvm
            .attach_current_thread(|env| -> Result<Option<String>> {
                let jarg1 = env.new_string(apk_id).context("new_string")?;
                let jarg2 = env.new_string(method_id).context("new_string")?;
                let result = env
                    .call_method(
                        &self.facade,
                        jni_str!("getMethodSmali"),
                        jni_sig!("(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;"),
                        &[JValue::Object(&jarg1), JValue::Object(&jarg2)],
                    )
                    .context("getMethodSmali call failed")?
                    .into_object()
                    .context("expected object")?;

                if result.is_null() {
                    return Ok(None);
                }
                let jstr = JString::cast_local(env, result).context("expected string")?;
                Ok(Some(
                    jstr.try_to_string(env)
                        .context("failed to convert Java string")?,
                ))
            })
            .context("attach")
    }

    // -- Bulk method data export ----------------------------------------------

    pub fn get_apk_method_data(&self, apk_id: &str) -> Result<Vec<MethodData>> {
        let bytes = self
            .call_bytes_method_1("getApkMethodDataProto", apk_id)?
            .context("getApkMethodDataProto returned null")?;
        let method_data =
            decode_message::<MethodDataList>(bytes.as_slice(), "deserialize method data list")?;
        validate_method_data(&method_data.items)?;
        Ok(method_data.items)
    }

    // -- Script execution -----------------------------------------------------

    pub fn evaluate_script(
        &self,
        script_path: &str,
        fingerprint_result_cap: i32,
        save_patched_apks: bool,
    ) -> Result<ExecutionResultResponse> {
        let bytes = self.call_bytes_method_3(
            "evaluateScriptProto",
            script_path,
            fingerprint_result_cap,
            save_patched_apks,
        )?;
        let execution_result = decode_message::<ExecutionResultResponse>(
            bytes.as_slice(),
            "deserialize execution result response",
        )?;
        validate_engine_events(&execution_result.events)?;
        Ok(execution_result)
    }

    pub fn close(&self) -> Result<()> {
        self.jvm
            .attach_current_thread(|env| -> Result<()> {
                env.call_method(&self.facade, jni_str!("close"), jni_sig!("()V"), &[])
                    .context("close call failed")?;
                Ok(())
            })
            .context("attach")
    }

    // -- Helpers --------------------------------------------------------------

    /// Call a `JniFacade` method that takes one String and returns a nullable byte array.
    fn call_bytes_method_1(&self, method: &str, arg: &str) -> Result<Option<Vec<u8>>> {
        self.jvm
            .attach_current_thread(|env| -> Result<Option<Vec<u8>>> {
                let jarg = env.new_string(arg).context("new_string")?;
                let name = JNIString::new(method);
                let result = env
                    .call_method(
                        &self.facade,
                        &name,
                        jni_sig!("(Ljava/lang/String;)[B"),
                        &[JValue::Object(&jarg)],
                    )
                    .with_context(|| format!("{method} call failed"))?
                    .into_object()
                    .context("expected object")?;

                if result.is_null() {
                    return Ok(None);
                }
                let jbytes = JByteArray::cast_local(env, result).context("expected byte array")?;
                Ok(Some(
                    env.convert_byte_array(&jbytes)
                        .context("convert_byte_array")?,
                ))
            })
            .context("attach")
    }

    fn call_bytes_method_3(
        &self,
        method: &str,
        arg: &str,
        number: i32,
        flag: bool,
    ) -> Result<Vec<u8>> {
        self.jvm
            .attach_current_thread(|env| -> Result<Vec<u8>> {
                let jarg = env.new_string(arg).context("new_string")?;
                let name = JNIString::new(method);
                let result = env
                    .call_method(
                        &self.facade,
                        &name,
                        jni_sig!("(Ljava/lang/String;IZ)[B"),
                        &[
                            JValue::Object(&jarg),
                            JValue::Int(number),
                            JValue::Bool(flag),
                        ],
                    )
                    .with_context(|| format!("{method} call failed"))?
                    .into_object()
                    .context("expected object")?;

                if result.is_null() {
                    anyhow::bail!("{method} returned null");
                }
                let jbytes = JByteArray::cast_local(env, result).context("expected byte array")?;
                env.convert_byte_array(&jbytes)
                    .context("convert_byte_array")
            })
            .context("attach")
    }
}

impl Drop for EngineJni {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

fn decode_message<M>(bytes: &[u8], context: &str) -> Result<M>
where
    M: Message + Default,
{
    M::decode(bytes).with_context(|| context.to_string())
}

fn validate_method_data(methods: &[MethodData]) -> Result<()> {
    for (method_index, method) in methods.iter().enumerate() {
        method
            .info
            .as_ref()
            .with_context(|| format!("method data[{method_index}] missing info"))?;
        let features = method
            .features
            .as_ref()
            .with_context(|| format!("method data[{method_index}] missing features"))?;
        features
            .signature
            .as_ref()
            .with_context(|| format!("method data[{method_index}] missing signature"))?;
        for (instruction_index, instruction) in features.instructions.iter().enumerate() {
            instruction.kind.as_ref().with_context(|| {
                format!("method data[{method_index}] instruction[{instruction_index}] missing kind")
            })?;
        }
    }

    Ok(())
}

fn validate_engine_events(events: &[EngineEvent]) -> Result<()> {
    for (event_index, event) in events.iter().enumerate() {
        match event
            .kind
            .as_ref()
            .with_context(|| format!("engine event[{event_index}] missing kind"))?
        {
            engine_event::Kind::ItemResult(item_result) => {
                let result = item_result.result.as_ref().with_context(|| {
                    format!("engine event[{event_index}] item_result missing result")
                })?;
                result.kind.as_ref().with_context(|| {
                    format!("engine event[{event_index}] item_result result missing kind")
                })?;
            }
            engine_event::Kind::RunCompleted(_)
            | engine_event::Kind::RunFailed(_)
            | engine_event::Kind::ScriptOutput(_)
            | engine_event::Kind::PatchedApkSaved(_)
            | engine_event::Kind::PatchedApkSaveFailed(_) => {}
        }
    }

    Ok(())
}
