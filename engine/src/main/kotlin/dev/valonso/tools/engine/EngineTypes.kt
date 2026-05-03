package dev.valonso.tools.engine

import dev.valonso.tools.engine.apk.LoadedApk

/**
 * Lightweight identifier for a loaded APK.
 */
data class ApkIdentity(
    val id: String,
    val sourceFilePath: String,
    val packageName: String,
    val packageVersion: String,
) {
    val friendlyName: String get() = "$packageName / $packageVersion"
    val fileName: String get() = sourceFilePath.substringAfterLast('/')
}

/**
 * Identifies one processable item from a script return value.
 * For scripts returning a single value, there is one item at index 0.
 * For scripts returning a List, each element becomes a separate item.
 */
data class ScriptItemId(
    val index: Int,
    val label: String,
    val kind: ScriptItemKind,
)

enum class ScriptItemKind {
    BytecodePatch,
    ResourcePatch,
    RawResourcePatch,
    Fingerprint,
    Generic,
    ;

    val isPatch: Boolean
        get() = this == BytecodePatch || this == ResourcePatch || this == RawResourcePatch
}

/**
 * Configuration for an engine execution run.
 */
data class EngineRunConfig(
    /** Maximum number of fingerprint matches to report per item per APK. */
    val fingerprintResultCap: Int = 8,
    /** Whether successful runs should also save patched APK outputs. */
    val savePatchedApks: Boolean = false,
)

/**
 * Retains patch values and APK references for a single engine execution
 * so patched APK saves can happen without re-running the script.
 */
internal class EngineRunSession(
    val patchGroupId: ScriptItemId,
    val patchValues: List<Any>,
    private val apksByApkId: Map<String, LoadedApk>,
) {
    fun apks(): Collection<LoadedApk> = apksByApkId.values
}
