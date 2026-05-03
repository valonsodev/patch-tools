package dev.valonso.tools.engine

import app.morphe.patcher.Fingerprint
import app.morphe.patcher.apk.ApkUtils.KeyStoreDetails
import app.morphe.patcher.apk.ApkUtils.applyTo
import app.morphe.patcher.apk.ApkUtils.signApk
import app.morphe.patcher.patch.BytecodePatch
import app.morphe.patcher.patch.Patch
import app.morphe.patcher.patch.RawResourcePatch
import app.morphe.patcher.patch.ResourcePatch
import dev.valonso.tools.engine.apk.ApkInputSupport
import dev.valonso.tools.engine.apk.LoadedApk
import dev.valonso.tools.engine.method.uniqueId
import dev.valonso.tools.engine.scripting.MorpheScriptingHost
import dev.valonso.tools.engine.scripting.ScriptOutputSink
import dev.valonso.tools.engine.scripting.ScriptEvalResult
import io.github.oshai.kotlinlogging.KotlinLogging
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.collect
import kotlinx.coroutines.withContext
import java.io.File
import java.util.UUID

private val logger = KotlinLogging.logger("MorpheEngine")

/**
 * Main entry point for the engine module.
 * Executes scripts against APKs and returns [EngineEvent]s.
 */
class MorpheEngine(
    private val scriptingHost: MorpheScriptingHost,
) {
    /**
     * Execute a script against provided APKs.
     * Patches are always executed together in a single patcher session.
     * Non-patch items (fingerprints, generics) are processed individually.
     */
    suspend fun execute(
        scriptPath: String,
        apks: List<LoadedApk>,
        config: EngineRunConfig = EngineRunConfig(),
    ): List<EngineEvent> {
        val events = mutableListOf<EngineEvent>()
        executeInternal(scriptPath, apks, config, events)
        return events
    }

    /**
     * Execute a script by loading APKs from file paths (used by CLI).
     * APKs are loaded, processed, then cleaned up.
     */
    suspend fun executeFromPaths(
        scriptPath: String,
        apkPaths: List<String>,
        config: EngineRunConfig = EngineRunConfig(),
    ): List<EngineEvent> {
        val tempDir = File(System.getProperty("java.io.tmpdir"), "morphe-engine-${System.currentTimeMillis()}")
        tempDir.mkdirs()
        val apks = mutableListOf<LoadedApk>()

        try {
            for (path in apkPaths) {
                val apkFile = File(path)
                if (!apkFile.exists()) {
                    return listOf(
                        EngineEvent.RunFailed(
                            "Android package file not found: $path. Supported formats: ${ApkInputSupport.supportedInputLabel()}",
                        ),
                    )
                }
                if (!ApkInputSupport.isSupported(apkFile)) {
                    return listOf(
                        EngineEvent.RunFailed(
                            "Unsupported Android package input: $path. Supported formats: ${ApkInputSupport.supportedInputLabel()}",
                        ),
                    )
                }
                apks.add(LoadedApk.load(apkFile, tempDir))
            }

            return execute(scriptPath, apks, config)
        } finally {
            apks.forEach { runCatching { it.dispose() } }
            runCatching { tempDir.deleteRecursively() }
        }
    }

    private suspend fun executeInternal(
        scriptPath: String,
        apks: List<LoadedApk>,
        config: EngineRunConfig,
        events: MutableList<EngineEvent>,
    ) {
        logger.info { "Engine execution started: script=$scriptPath, apks=${apks.size}" }

        val scriptValue =
            when (val evalResult = captureScriptOutput(events = events) { scriptingHost.evaluateScriptFile(scriptPath) }) {
                is ScriptEvalResult.Failure -> {
                    events +=
                        EngineEvent.RunFailed(
                            error = evalResult.errors.joinToString("\n"),
                            exception = evalResult.exception,
                        )
                    return
                }
                is ScriptEvalResult.Success -> evalResult.value
            }

        val allClassified = classifyScriptResult(scriptValue)
        if (allClassified.isEmpty()) {
            events += EngineEvent.RunCompleted(
                totalItems = 0,
                totalApks = apks.size,
                errors = listOf("Script returned null or empty result."),
            )
            return
        }

        val patchItems = allClassified.filter { it.id.kind.isPatch }
        val nonPatchItems = allClassified.filter { !it.id.kind.isPatch }

        // Build a single item ID for the combined patch group
        val patchGroupId = if (patchItems.isNotEmpty()) {
            val label = if (patchItems.size == 1) patchItems.single().id.label
                        else "Patches: ${patchItems.joinToString(", ") { it.id.label }}"
            ScriptItemId(index = 0, label = label, kind = patchItems.first().id.kind)
        } else null

        val reportedItems = buildList {
            if (patchGroupId != null) add(patchGroupId)
            addAll(nonPatchItems.map { it.id })
        }

        val runSession = if (config.savePatchedApks && patchGroupId != null) {
            EngineRunSession(
                patchGroupId = patchGroupId,
                patchValues = patchItems.map { it.value },
                apksByApkId = apks.associateBy { it.id },
            )
        } else null

        events += EngineEvent.ItemsResolved(
            items = reportedItems,
            apks = apks.map { it.identity },
        )

        val errors = mutableListOf<String>()

        for (apk in apks) {
            // Always run all patches together in one session
            if (patchGroupId != null) {
                events += EngineEvent.ItemProcessingStarted(patchGroupId, apk.identity)
                val addResult: (EngineResult) -> Unit = { result ->
                    events += EngineEvent.ItemResult(patchGroupId, apk.identity, result)
                }
                try {
                    captureScriptOutput(itemId = patchGroupId, apk = apk.identity, events = events) {
                        processPatches(patchItems, apk, scriptPath, addResult)
                    }
                } catch (e: Exception) {
                    logger.error(e) { "Error processing patches against ${apk.identity.friendlyName}" }
                    errors.add("${patchGroupId.label} / ${apk.identity.friendlyName}: ${e.message}")
                    addResult(EngineResult.ItemError(message = e.message ?: "Unknown error", exception = e))
                }
                events += EngineEvent.ItemProcessingCompleted(patchGroupId, apk.identity)
            }

            // Process non-patch items individually
            for (item in nonPatchItems) {
                events += EngineEvent.ItemProcessingStarted(item.id, apk.identity)
                val addResult: (EngineResult) -> Unit = { result ->
                    events += EngineEvent.ItemResult(item.id, apk.identity, result)
                }
                try {
                    captureScriptOutput(itemId = item.id, apk = apk.identity, events = events) {
                        processNonPatchItem(item, apk, scriptPath, config, addResult)
                    }
                } catch (e: Exception) {
                    logger.error(e) { "Error processing ${item.id.label} against ${apk.identity.friendlyName}" }
                    errors.add("${item.id.label} / ${apk.identity.friendlyName}: ${e.message}")
                    addResult(EngineResult.ItemError(message = e.message ?: "Unknown error", exception = e))
                }
                events += EngineEvent.ItemProcessingCompleted(item.id, apk.identity)
            }
        }

        val runCompleted = EngineEvent.RunCompleted(
            totalItems = reportedItems.size,
            totalApks = apks.size,
            errors = errors,
        )
        events += runCompleted
        if (runSession != null && runCompleted.totalItems > 0 && runCompleted.errors.isEmpty()) {
            appendPatchedApkSaveEvents(runSession, events)
        }
    }

    private suspend fun appendPatchedApkSaveEvents(
        session: EngineRunSession,
        events: MutableList<EngineEvent>,
    ) {
        session.apks().forEach { apk ->
            val event =
                runCatching {
                    val patches = session.patchValues.map {
                        it as? Patch<*> ?: error("Expected Patch, got ${it::class.simpleName}")
                    }
                    logger.info { "Saving patched APK: item=${session.patchGroupId.label}, apk=${apk.identity.friendlyName}" }
                    val outputFile = savePatchedApk(apk, patches, session.patchGroupId.label)
                    EngineEvent.PatchedApkSaved(
                        apk = apk.identity,
                        itemId = session.patchGroupId,
                        apkPath = outputFile.absolutePath,
                    )
                }.getOrElse { error ->
                    EngineEvent.PatchedApkSaveFailed(
                        apk = apk.identity,
                        itemId = session.patchGroupId,
                        error = error.message ?: "Unknown save error",
                    )
                }
            events += event
        }
    }

    /**
     * Runs all patches together: re-evaluates the script to get fresh stateful
     * fingerprint/patch instances, then runs the full patch set in one patcher
     * session while collecting bytecode and resource diffs.
     */
    private suspend fun processPatches(
        patchItems: List<ClassifiedScriptItem>,
        apk: LoadedApk,
        scriptPath: String,
        addResult: (EngineResult) -> Unit,
    ) {
        val freshItems = evaluateFreshItems(scriptPath, addResult) ?: return
        val freshPatches = patchItems.mapNotNull { freshItems.getOrNull(it.id.index) }
        if (freshPatches.size != patchItems.size) {
            addResult(
                EngineResult.ItemError(
                    "Expected ${patchItems.size} patches but found ${freshPatches.size} in re-evaluated script",
                ),
            )
            return
        }

        val patches = freshPatches.map { it.value as Patch<*> }
        val result = apk.runPatchesAndCollectDiffs(patches)
        addResult(EngineResult.BytecodePatchResult(result.methodDiffs, result.classDiffs))
        addResult(EngineResult.ResourcePatchResult(result.resourceChanges))
    }

    private suspend fun processNonPatchItem(
        item: ClassifiedScriptItem,
        apk: LoadedApk,
        scriptPath: String,
        config: EngineRunConfig,
        addResult: (EngineResult) -> Unit,
    ) {
        val freshItems = evaluateFreshItems(scriptPath, addResult) ?: return
        val freshItem = freshItems.getOrNull(item.id.index)
        if (freshItem == null) {
            addResult(EngineResult.ItemError("Item at index ${item.id.index} not found in re-evaluated script"))
            return
        }

        when (item.id.kind) {
            ScriptItemKind.Fingerprint -> {
                val matches = apk.searchFingerprint(freshItem.value as Fingerprint)
                    .map { method ->
                        MatchedMethod(
                            uniqueId = method.uniqueId,
                            definingClass = method.definingClass,
                            methodName = method.name,
                            returnType = method.returnType,
                            parameters = method.parameterTypes.map { it.toString() },
                        )
                    }
                    .take(config.fingerprintResultCap)
                addResult(EngineResult.FingerprintMatches(matches))
            }

            ScriptItemKind.Generic -> {
                addResult(
                    EngineResult.GenericResult(
                        typeName = freshItem.value::class.simpleName ?: "Unknown",
                        textRepresentation = freshItem.value.toString(),
                    ),
                )
            }

            else -> {}
        }
    }

    private suspend fun evaluateFreshItems(
        scriptPath: String,
        addResult: (EngineResult) -> Unit,
    ): List<ClassifiedScriptItem>? =
        when (val freshResult = scriptingHost.evaluateScriptFile(scriptPath)) {
            is ScriptEvalResult.Success -> classifyScriptResult(freshResult.value)
            is ScriptEvalResult.Failure -> {
                addResult(
                    EngineResult.ItemError(
                        message = "Script re-evaluation failed: ${freshResult.errors.joinToString("; ")}",
                        exception = freshResult.exception,
                    ),
                )
                null
            }
        }

    private suspend fun <T> captureScriptOutput(
        itemId: ScriptItemId? = null,
        apk: ApkIdentity? = null,
        events: MutableList<EngineEvent>,
        block: suspend () -> T,
    ): T {
        val output = mutableListOf<String>()

        try {
            return ScriptOutputSink.withCallback(output::add, block)
        } finally {
            for (line in output) {
                events += EngineEvent.ScriptOutput(text = line, itemId = itemId, apk = apk)
            }
        }
    }
}

internal data class ClassifiedScriptItem(
    val id: ScriptItemId,
    val value: Any,
)

internal fun classifyScriptResult(value: Any?): List<ClassifiedScriptItem> {
    if (value == null || value == Unit) return emptyList()

    val items: List<Any> =
        when (value) {
            is List<*> -> value.filterNotNull()
            else -> listOf(value)
        }

    return items.mapIndexed { index, item ->
        val kind =
            when (item) {
                is BytecodePatch -> ScriptItemKind.BytecodePatch
                is ResourcePatch -> ScriptItemKind.ResourcePatch
                is RawResourcePatch -> ScriptItemKind.RawResourcePatch
                is Fingerprint -> ScriptItemKind.Fingerprint
                else -> ScriptItemKind.Generic
            }
        val label =
            when (item) {
                is BytecodePatch -> item.name?.ifBlank { null } ?: "BytecodePatch #${index + 1}"
                is ResourcePatch -> item.name?.ifBlank { null } ?: "ResourcePatch #${index + 1}"
                is RawResourcePatch -> item.name?.ifBlank { null } ?: "RawResourcePatch #${index + 1}"
                is Fingerprint ->
                    item::class.simpleName?.takeIf { !it.startsWith("Fingerprint") }
                        ?: item.name?.ifBlank { null }
                        ?: "Fingerprint #${index + 1}"
                else -> "${item::class.simpleName ?: "Unknown"} #${index + 1}"
            }
        ClassifiedScriptItem(
            id = ScriptItemId(index = index, label = label, kind = kind),
            value = item,
        )
    }
}

private const val MORPHE_KEY_ALIAS = "Morphe"
private const val MORPHE_KEY_PASSWORD = "Morphe"
private const val SIGNER_NAME = MORPHE_KEY_ALIAS

internal suspend fun savePatchedApk(
    apk: LoadedApk,
    patches: List<Patch<*>>,
    patchLabel: String,
): File =
    withContext(Dispatchers.IO) {
        val outputFile = resolveOutputFile(apk, patchLabel)
        val workingRoot =
            File(System.getProperty("java.io.tmpdir"))
                .resolve("patch-tools")
                .resolve("patched-apk-builds")
                .resolve(UUID.randomUUID().toString())
                .apply { mkdirs() }

        try {
            val patcherTempDir = workingRoot.resolve("patcher")
            val unsignedApk = workingRoot.resolve("unsigned.apk")

            apk.createIsolatedPatcher(patcherTempDir).use { patcher ->
                patcher += patches.toSet()
                patcher().collect { patchResult ->
                    patchResult.exception?.let { exception ->
                        val patchName = patchResult.patch.name ?: patchLabel.ifBlank { "patch" }
                        throw IllegalStateException(
                            "Patch \"$patchName\" failed while building the APK: ${exception.message}",
                            exception,
                        )
                    }
                }

                apk.workingFile.copyTo(unsignedApk, overwrite = true)
                val patcherResult = patcher.get()
                patcherResult.applyTo(unsignedApk)
            }

            if (outputFile.exists()) {
                outputFile.delete()
            }
            outputFile.parentFile?.mkdirs()

            signApk(
                unsignedApk,
                outputFile,
                SIGNER_NAME,
                signingKeyStoreDetails(outputFile),
            )
            logger.info { "Saved patched APK to ${outputFile.absolutePath}" }
            outputFile
        } finally {
            workingRoot.deleteRecursively()
        }
    }

private fun signingKeyStoreDetails(outputFile: File): KeyStoreDetails {
    val parent = outputFile.parentFile ?: error("Output file must have a parent directory")
    parent.mkdirs()
    val keyStoreFile = parent.resolve("${outputFile.nameWithoutExtension}.keystore")

    return KeyStoreDetails(
        keyStore = keyStoreFile,
        keyStorePassword = null,
        alias = MORPHE_KEY_ALIAS,
        password = MORPHE_KEY_PASSWORD,
    )
}

private fun resolveOutputFile(
    apk: LoadedApk,
    patchLabel: String,
): File {
    val outputDir =
        File(System.getProperty("java.io.tmpdir"))
            .resolve("patch-tools")
            .resolve("exported-apks")
            .apply { mkdirs() }

    val versionSegment = sanitizeFileNameSegment(apk.packageMetadata.versionName.ifBlank { "patched" })
    val patchSegment = sanitizeFileNameSegment(patchLabel).takeIf(String::isNotBlank)
    val suffix = patchSegment?.let { "-$it" }.orEmpty()
    val fileName = "${apk.sourceFile.nameWithoutExtension}-Morphe-$versionSegment$suffix.apk"
    return outputDir.resolve(fileName)
}

private fun sanitizeFileNameSegment(value: String): String {
    val sanitized =
        value
            .trim()
            .replace(Regex("[^A-Za-z0-9._-]+"), "_")
            .trim('_', '.', '-')
            .ifBlank { "patched" }

    return sanitized.take(48)
}
