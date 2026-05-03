package dev.valonso.tools.engine.jni

import dev.valonso.tools.engine.ApkIdentity
import dev.valonso.tools.engine.ClassChangeKind
import dev.valonso.tools.engine.ClassDiff
import dev.valonso.tools.engine.EngineEvent
import dev.valonso.tools.engine.EngineResult
import dev.valonso.tools.engine.EngineRunConfig
import dev.valonso.tools.engine.MatchedMethod
import dev.valonso.tools.engine.MethodChangeKind
import dev.valonso.tools.engine.MethodDiff
import dev.valonso.tools.engine.ResourceChange
import dev.valonso.tools.engine.ResourceChangeKind
import dev.valonso.tools.engine.fingerprint.InstructionFeature
import dev.valonso.tools.engine.fingerprint.MethodFeatures
import dev.valonso.tools.engine.fingerprint.MethodSignature
import dev.valonso.tools.engine.proto.ApkIdentity as ApkIdentityProto
import dev.valonso.tools.engine.proto.ApkStatus as ApkStatusProto
import dev.valonso.tools.engine.proto.ClassDiffDto as ClassDiffDtoProto
import dev.valonso.tools.engine.proto.EngineEvent as EngineEventProto
import dev.valonso.tools.engine.proto.EngineEventItemResult as EngineEventItemResultProto
import dev.valonso.tools.engine.proto.EngineEventPatchedApkSaved as EngineEventPatchedApkSavedProto
import dev.valonso.tools.engine.proto.EngineEventPatchedApkSaveFailed as EngineEventPatchedApkSaveFailedProto
import dev.valonso.tools.engine.proto.EngineEventRunCompleted as EngineEventRunCompletedProto
import dev.valonso.tools.engine.proto.EngineEventRunFailed as EngineEventRunFailedProto
import dev.valonso.tools.engine.proto.EngineEventScriptOutput as EngineEventScriptOutputProto
import dev.valonso.tools.engine.proto.EngineResultBytecodeDiffResult as EngineResultBytecodeDiffResultProto
import dev.valonso.tools.engine.proto.EngineResult as EngineResultProto
import dev.valonso.tools.engine.proto.EngineResultFingerprintMatches as EngineResultFingerprintMatchesProto
import dev.valonso.tools.engine.proto.EngineResultGenericResult as EngineResultGenericResultProto
import dev.valonso.tools.engine.proto.EngineResultItemError as EngineResultItemErrorProto
import dev.valonso.tools.engine.proto.EngineResultResourcePatchResult as EngineResultResourcePatchResultProto
import dev.valonso.tools.engine.proto.ExecutionResultResponse as ExecutionResultResponseProto
import dev.valonso.tools.engine.proto.InstructionCheckCast as InstructionCheckCastProto
import dev.valonso.tools.engine.proto.InstructionFeature as InstructionFeatureProto
import dev.valonso.tools.engine.proto.InstructionFieldAccess as InstructionFieldAccessProto
import dev.valonso.tools.engine.proto.InstructionInstanceOf as InstructionInstanceOfProto
import dev.valonso.tools.engine.proto.InstructionLiteral as InstructionLiteralProto
import dev.valonso.tools.engine.proto.InstructionMethodCall as InstructionMethodCallProto
import dev.valonso.tools.engine.proto.InstructionNewInstance as InstructionNewInstanceProto
import dev.valonso.tools.engine.proto.InstructionStringConst as InstructionStringConstProto
import dev.valonso.tools.engine.proto.MatchedMethodDto as MatchedMethodDtoProto
import dev.valonso.tools.engine.proto.MethodData as MethodDataProto
import dev.valonso.tools.engine.proto.MethodDataList as MethodDataListProto
import dev.valonso.tools.engine.proto.MethodDiffDto as MethodDiffDtoProto
import dev.valonso.tools.engine.proto.MethodFeaturesDto as MethodFeaturesDtoProto
import dev.valonso.tools.engine.proto.MethodInfoDto as MethodInfoDtoProto
import dev.valonso.tools.engine.proto.MethodSignatureDto as MethodSignatureDtoProto
import dev.valonso.tools.engine.proto.ResourceChangeDto as ResourceChangeDtoProto
import dev.valonso.tools.engine.scripting.MorpheScriptingHost
import dev.valonso.tools.engine.session.EngineSession
import dev.valonso.tools.engine.session.MethodInfo
import dev.valonso.tools.engine.session.toMethodInfo
import kotlinx.coroutines.runBlocking
import java.io.File

/**
 * JNI-friendly facade over [EngineSession].
 *
 * Data crosses the JNI boundary as protobuf-encoded bytes.
 */
class JniFacade {
    data class ApkStatus(
        val identity: ApkIdentity,
        val classCount: Int,
        val methodCount: Int,
    )

    data class MethodData(
        val info: MethodInfo,
        val signature: MethodSignature,
        val instructions: List<InstructionFeature>,
    )

    private val scriptingHost = MorpheScriptingHost()
    private val session = EngineSession(scriptingHost)

    // -- APK lifecycle --------------------------------------------------------

    fun loadApkProto(path: String): ByteArray? {
        val identity = runBlocking { session.loadApk(File(path)) } ?: return null
        return ApkIdentityProto.ADAPTER.encode(identity.toProto())
    }

    fun unloadApk(apkId: String) {
        session.unloadApk(apkId)
    }

    fun getApkStatusProto(apkId: String): ByteArray? {
        return runBlocking {
            session.withResolvedApk(apkId) { apk ->
                ApkStatusProto.ADAPTER.encode(
                    ApkStatusProto(
                        identity = apk.identity.toProto(),
                        class_count = apk.classes.size,
                        method_count = apk.methods.size,
                    ),
                )
            }
        }
    }

    // -- Bulk method data export ----------------------------------------------

    fun getApkMethodDataProto(apkId: String): ByteArray? {
        return runBlocking {
            session.withResolvedApk(apkId) { apk ->
                val methods =
                    apk.methods.map { method ->
                        val features = MethodFeatures.extract(method)
                        MethodData(
                            info = method.toMethodInfo(),
                            signature = features.signature,
                            instructions = features.instructions,
                        )
                    }
                MethodDataListProto.ADAPTER.encode(
                    MethodDataListProto(items = methods.map { it.toProto() }),
                )
            }
        }
    }

    // -- Method info ----------------------------------------------------------

    fun getMethodSmali(apkId: String, methodId: String): String? =
        session.getMethodSmali(apkId, methodId)

    // -- Script execution -----------------------------------------------------

    fun evaluateScriptProto(
        scriptPath: String,
        fingerprintResultCap: Int,
        savePatchedApks: Boolean,
    ): ByteArray =
        runBlocking {
            val config =
                EngineRunConfig(
                    fingerprintResultCap = fingerprintResultCap,
                    savePatchedApks = savePatchedApks,
                )
            val events = session.execute(scriptPath, config).filter {
                it is EngineEvent.ItemResult ||
                    it is EngineEvent.ScriptOutput ||
                    it is EngineEvent.RunCompleted ||
                    it is EngineEvent.RunFailed ||
                    it is EngineEvent.PatchedApkSaved ||
                    it is EngineEvent.PatchedApkSaveFailed
            }
            ExecutionResultResponseProto.ADAPTER.encode(
                ExecutionResultResponseProto(
                    events = events.mapNotNull { it.toProto() },
                ),
            )
        }

    // -- Lifecycle -------------------------------------------------------------

    fun close() {
        session.close()
    }
}

// -- Proto mapping extensions -------------------------------------------------

private fun ApkIdentity.toProto(): ApkIdentityProto =
    ApkIdentityProto(
        id = id,
        source_file_path = sourceFilePath,
        package_name = packageName,
        package_version = packageVersion,
    )

private fun JniFacade.MethodData.toProto(): MethodDataProto =
    MethodDataProto(
        info = info.toProto(),
        features =
            MethodFeaturesDtoProto(
                signature = signature.toProto(),
                instructions = instructions.map { it.toProto() },
            ),
    )

private fun MethodInfo.toProto(): MethodInfoDtoProto =
    MethodInfoDtoProto(
        unique_id = uniqueId,
        defining_class = definingClass,
        name = name,
        return_type = returnType,
        parameters = parameters,
        access_flags = accessFlags,
        class_name = className,
        java_return_type = javaReturnType,
        java_parameter_types = javaParameterTypes,
        java_access_flags = javaAccessFlags,
        short_id = shortId,
        java_signature = javaSignature,
    )

private fun MethodSignature.toProto(): MethodSignatureDtoProto =
    MethodSignatureDtoProto(
        return_type = returnType,
        access_flags = accessFlags,
        parameters = parameters,
    )

private fun InstructionFeature.toProto(): InstructionFeatureProto =
    when (this) {
        is InstructionFeature.Literal ->
            InstructionFeatureProto(
                index = index,
                literal = InstructionLiteralProto(value_ = value),
            )

        is InstructionFeature.StringConst ->
            InstructionFeatureProto(
                index = index,
                string_const = InstructionStringConstProto(patch_tools_string = string),
            )

        is InstructionFeature.MethodCall ->
            InstructionFeatureProto(
                index = index,
                method_call =
                    InstructionMethodCallProto(
                        defining_class = definingClass,
                        same_defining_class = sameDefiningClass,
                        use_this_defining_class = useThisDefiningClass,
                        name = name,
                        parameters = parameters,
                        return_type = returnType,
                    ),
            )

        is InstructionFeature.FieldAccess ->
            InstructionFeatureProto(
                index = index,
                field_access =
                    InstructionFieldAccessProto(
                        defining_class = definingClass,
                        same_defining_class = sameDefiningClass,
                        use_this_defining_class = useThisDefiningClass,
                        name = name,
                        field_type = `type`,
                    ),
            )

        is InstructionFeature.NewInstance ->
            InstructionFeatureProto(
                index = index,
                new_instance = InstructionNewInstanceProto(instance_type = `type`),
            )

        is InstructionFeature.InstanceOf ->
            InstructionFeatureProto(
                index = index,
                instance_of = InstructionInstanceOfProto(instance_type = `type`),
            )

        is InstructionFeature.CheckCast ->
            InstructionFeatureProto(
                index = index,
                check_cast = InstructionCheckCastProto(cast_type = `type`),
            )
    }

private fun EngineEvent.toProto(): EngineEventProto? =
    when (this) {
        is EngineEvent.ItemResult ->
            EngineEventProto(
                item_result =
                    EngineEventItemResultProto(
                        item_id = itemId.label,
                        apk_id = apk.friendlyName,
                        result = result.toProto(),
                    ),
            )

        is EngineEvent.RunCompleted ->
            EngineEventProto(
                run_completed =
                    EngineEventRunCompletedProto(
                        total_items = totalItems,
                        total_apks = totalApks,
                        errors = errors,
                    ),
            )

        is EngineEvent.RunFailed ->
            EngineEventProto(
                run_failed = EngineEventRunFailedProto(error = error),
            )

        is EngineEvent.ScriptOutput ->
            EngineEventProto(
                script_output =
                    EngineEventScriptOutputProto(
                        text = text,
                        item_id = itemId?.label,
                        apk_id = apk?.id,
                        apk_label = apk?.friendlyName,
                    ),
            )

        is EngineEvent.PatchedApkSaved ->
            EngineEventProto(
                patched_apk_saved =
                    EngineEventPatchedApkSavedProto(
                        apk_path = apkPath,
                        apk_id = apk.id,
                        item_id = itemId.label,
                        apk_label = apk.friendlyName,
                    ),
            )

        is EngineEvent.PatchedApkSaveFailed ->
            EngineEventProto(
                patched_apk_save_failed =
                    EngineEventPatchedApkSaveFailedProto(
                        apk_id = apk.id,
                        item_id = itemId?.label,
                        error = error,
                        apk_label = apk.friendlyName,
                    ),
            )

        else -> null
    }

private fun EngineResult.toProto(): EngineResultProto =
    when (this) {
        is EngineResult.FingerprintMatches ->
            EngineResultProto(
                fingerprint_matches =
                    EngineResultFingerprintMatchesProto(
                        methods = methods.map { it.toProto() },
                    ),
            )

        is EngineResult.BytecodePatchResult ->
            EngineResultProto(
                bytecode_diff_result =
                    EngineResultBytecodeDiffResultProto(
                        method_diffs = methodDiffs.map { it.toProto() },
                        class_diffs = classDiffs.map { it.toProto() },
                    ),
            )

        is EngineResult.ResourcePatchResult ->
            EngineResultProto(
                resource_patch_result =
                    EngineResultResourcePatchResultProto(
                        resource_changes = resourceChanges.map { it.toProto() },
                    ),
            )

        is EngineResult.GenericResult ->
            EngineResultProto(
                generic_result =
                    EngineResultGenericResultProto(
                        type_name = typeName,
                        text_representation = textRepresentation,
                    ),
            )

        is EngineResult.ItemError ->
            EngineResultProto(
                item_error = EngineResultItemErrorProto(message = message),
            )
    }

private fun MatchedMethod.toProto(): MatchedMethodDtoProto =
    MatchedMethodDtoProto(
        unique_id = uniqueId,
        defining_class = definingClass,
        method_name = methodName,
        return_type = returnType,
        parameters = parameters,
    )

private fun MethodDiff.toProto(): MethodDiffDtoProto =
    MethodDiffDtoProto(
        method_id = methodId,
        original_smali = originalSmali,
        modified_smali = modifiedSmali,
        change_kind = changeKind.toProto(),
    )

private fun ClassDiff.toProto(): ClassDiffDtoProto =
    ClassDiffDtoProto(
        class_type = classType,
        change_kind = changeKind.toProto(),
        original_header = originalHeader,
        modified_header = modifiedHeader,
    )

private fun ResourceChange.toProto(): ResourceChangeDtoProto =
    ResourceChangeDtoProto(
        relative_path = relativePath,
        kind = kind.toProto(),
        original_content = originalContent,
        modified_content = modifiedContent,
        original_hash = originalHash,
        modified_hash = modifiedHash,
    )

private fun MethodChangeKind.toProto(): MethodDiffDtoProto.ChangeKind =
    when (this) {
        MethodChangeKind.Modified -> MethodDiffDtoProto.ChangeKind.MODIFIED
        MethodChangeKind.Added -> MethodDiffDtoProto.ChangeKind.ADDED
        MethodChangeKind.Deleted -> MethodDiffDtoProto.ChangeKind.DELETED
    }

private fun ClassChangeKind.toProto(): ClassDiffDtoProto.ChangeKind =
    when (this) {
        ClassChangeKind.Added -> ClassDiffDtoProto.ChangeKind.ADDED
        ClassChangeKind.Modified -> ClassDiffDtoProto.ChangeKind.MODIFIED
    }

private fun ResourceChangeKind.toProto(): ResourceChangeDtoProto.Kind =
    when (this) {
        ResourceChangeKind.Added -> ResourceChangeDtoProto.Kind.ADDED
        ResourceChangeKind.Modified -> ResourceChangeDtoProto.Kind.MODIFIED
        ResourceChangeKind.Deleted -> ResourceChangeDtoProto.Kind.DELETED
    }
