package dev.valonso.tools.engine

/**
 * Events emitted by the engine during script execution.
 * Consumers receive these as a single ordered list.
 */
sealed interface EngineEvent {
    /** Script has been evaluated and resolved into processable items. */
    data class ItemsResolved(
        val items: List<ScriptItemId>,
        val apks: List<ApkIdentity>,
    ) : EngineEvent

    /** Processing of a specific script item against a specific APK has begun. */
    data class ItemProcessingStarted(
        val itemId: ScriptItemId,
        val apk: ApkIdentity,
    ) : EngineEvent

    /** A single result from processing one script item against one APK. */
    data class ItemResult(
        val itemId: ScriptItemId,
        val apk: ApkIdentity,
        val result: EngineResult,
    ) : EngineEvent

    /** All results for a (scriptItem, apk) pair have been emitted. */
    data class ItemProcessingCompleted(
        val itemId: ScriptItemId,
        val apk: ApkIdentity,
    ) : EngineEvent

    /** Output produced by script-authored println calls. */
    data class ScriptOutput(
        val text: String,
        val itemId: ScriptItemId? = null,
        val apk: ApkIdentity? = null,
    ) : EngineEvent

    /** Entire engine run has completed. */
    data class RunCompleted(
        val totalItems: Int,
        val totalApks: Int,
        val errors: List<String>,
    ) : EngineEvent

    /** A fatal error that stops the entire run. */
    data class RunFailed(
        val error: String,
        val exception: Throwable? = null,
    ) : EngineEvent

    /** A patched APK was saved successfully after the run completed. */
    data class PatchedApkSaved(
        val apk: ApkIdentity,
        val itemId: ScriptItemId,
        val apkPath: String,
    ) : EngineEvent

    /** Saving a patched APK failed after the run completed. */
    data class PatchedApkSaveFailed(
        val apk: ApkIdentity,
        val error: String,
        val itemId: ScriptItemId? = null,
    ) : EngineEvent
}

/**
 * The actual result data emitted by the engine.
 * Each [EngineEvent.ItemResult] carries one of these.
 */
sealed interface EngineResult {
    data class FingerprintMatches(
        val methods: List<MatchedMethod>,
    ) : EngineResult

    data class BytecodePatchResult(
        val methodDiffs: List<MethodDiff>,
        val classDiffs: List<ClassDiff>,
    ) : EngineResult

    data class ResourcePatchResult(
        val resourceChanges: List<ResourceChange>,
    ) : EngineResult

    data class GenericResult(
        val typeName: String,
        val textRepresentation: String,
    ) : EngineResult

    data class ItemError(
        val message: String,
        val exception: Throwable? = null,
    ) : EngineResult
}

enum class MethodChangeKind {
    Modified,
    Added,
    Deleted,
}

data class MethodDiff(
    val methodId: String,
    val originalSmali: String,
    val modifiedSmali: String,
    val changeKind: MethodChangeKind = MethodChangeKind.Modified,
)

enum class ClassChangeKind {
    Added,
    Modified,
}

data class ClassDiff(
    val classType: String,
    val changeKind: ClassChangeKind,
    val originalHeader: String,
    val modifiedHeader: String,
)

enum class ResourceChangeKind {
    Added,
    Modified,
    Deleted,
}

data class ResourceChange(
    val relativePath: String,
    val kind: ResourceChangeKind,
    val originalContent: String? = null,
    val modifiedContent: String? = null,
    val originalHash: String? = null,
    val modifiedHash: String? = null,
)

data class MatchedMethod(
    val uniqueId: String,
    val definingClass: String,
    val methodName: String,
    val returnType: String,
    val parameters: List<String>,
)

data class BytecodeDiffResult(
    val methodDiffs: List<MethodDiff>,
    val classDiffs: List<ClassDiff>,
)

data class PatchDiffResult(
    val methodDiffs: List<MethodDiff>,
    val classDiffs: List<ClassDiff>,
    val resourceChanges: List<ResourceChange>,
)
