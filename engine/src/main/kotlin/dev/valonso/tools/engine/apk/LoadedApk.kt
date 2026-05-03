package dev.valonso.tools.engine.apk

import app.morphe.patcher.Fingerprint
import app.morphe.patcher.PackageMetadata
import app.morphe.patcher.Patcher
import app.morphe.patcher.PatcherConfig
import app.morphe.patcher.dex.DexReadWrite
import app.morphe.patcher.patch.BytecodePatch
import app.morphe.patcher.patch.BytecodePatchContext
import app.morphe.patcher.patch.Patch
import app.morphe.patcher.patch.ResourcePatch
import app.morphe.patcher.patch.bytecodePatch
import app.morphe.patcher.patch.rawResourcePatch
import app.morphe.patcher.patch.resourcePatch
import app.morphe.patcher.util.proxy.mutableTypes.MutableClass
import com.android.tools.smali.dexlib2.iface.ClassDef
import com.android.tools.smali.dexlib2.iface.Method
import dev.valonso.tools.engine.ApkIdentity
import dev.valonso.tools.engine.BytecodeDiffResult
import dev.valonso.tools.engine.ClassChangeKind
import dev.valonso.tools.engine.ClassDiff
import dev.valonso.tools.engine.MethodChangeKind
import dev.valonso.tools.engine.MethodDiff
import dev.valonso.tools.engine.PatchDiffResult
import dev.valonso.tools.engine.ResourceChange
import dev.valonso.tools.engine.ResourceChangeKind
import dev.valonso.tools.engine.method.javaSignature
import dev.valonso.tools.engine.method.uniqueId
import dev.valonso.tools.engine.patcher.deletedResourcePaths
import dev.valonso.tools.engine.smali.toHeaderSmaliString
import dev.valonso.tools.engine.smali.toSmaliString
import io.github.oshai.kotlinlogging.KotlinLogging
import kotlinx.coroutines.flow.collect
import java.io.File
import java.security.MessageDigest
import java.util.UUID

class LoadedApk(
    val sourceFile: File,
    val workingFile: File,
    private val temporaryInputPath: File?,
    patcherTemporaryFilesPath: File,
    val contentHash: String,
) {
    companion object {
        private val logger = KotlinLogging.logger("LoadedApk")

        /**
         * Apktool resource decoding inside [Patcher] construction is not thread-safe.
         * Serialize all Patcher creation to avoid CharsetDecoder state corruption.
         */
        private val patcherCreationLock = Any()

        private fun createPatcher(config: PatcherConfig): Patcher =
            synchronized(patcherCreationLock) {
                Patcher(config)
            }

        fun computeContentHash(sourceFile: File): String =
            MessageDigest
                .getInstance("SHA-256")
                .let { digest ->
                    sourceFile.inputStream().buffered().use { input ->
                        val buffer = ByteArray(DEFAULT_BUFFER_SIZE)
                        while (true) {
                            val bytesRead = input.read(buffer)
                            if (bytesRead < 0) break
                            if (bytesRead > 0) {
                                digest.update(buffer, 0, bytesRead)
                            }
                        }
                    }
                    digest.digest().joinToString("") { "%02x".format(it) }
                }

        fun load(
            sourceFile: File,
            patcherTemporaryFilesPath: File,
            contentHash: String = computeContentHash(sourceFile),
        ): LoadedApk {
            val preparedInput = ApkInputSupport.prepare(sourceFile, patcherTemporaryFilesPath)
            return try {
                LoadedApk(
                    sourceFile = preparedInput.sourceFile,
                    workingFile = preparedInput.workingFile,
                    temporaryInputPath = preparedInput.temporaryInputPath,
                    patcherTemporaryFilesPath = patcherTemporaryFilesPath,
                    contentHash = contentHash,
                )
            } catch (exception: Exception) {
                preparedInput.temporaryInputPath?.deleteRecursively()
                throw exception
            }
        }
    }

    var packageMetadata: PackageMetadata
    var patcher: Patcher
    var patcherTempPath: File
    private val classList: List<ClassDef>
    val classes: List<ClassDef>
        get() = classList

    private val methodList: List<Method>
    val methods: List<Method>
        get() = methodList

    private val classesByType: Map<String, ClassDef>
    private val methodsById: Map<String, Method>
    private val methodsByJavaSignature: Map<String, Method>

    val friendlyName: String
        get() = "${this.packageMetadata.packageName} / ${this.packageMetadata.versionName}"

    private val _id: String by lazy {
        MessageDigest
            .getInstance("SHA-256")
            .digest(this.sourceFile.absolutePath.toByteArray())
            .joinToString("") { "%02x".format(it) }
    }
    val id: String
        get() = _id

    val identity: ApkIdentity
        get() =
            ApkIdentity(
                id = id,
                sourceFilePath = sourceFile.absolutePath,
                packageName = packageMetadata.packageName,
                packageVersion = packageMetadata.versionName,
            )

    init {
        //        setLogLevel(logger, Level.OFF)
        logger.info { "Initializing LoadedApk from ${this.sourceFile.absolutePath}" }
        if (this.workingFile != this.sourceFile) {
            logger.info { "Using merged working APK at ${this.workingFile.absolutePath}" }
        }
        this.patcherTempPath = File(patcherTemporaryFilesPath, UUID.randomUUID().toString())
        logger.debug { "Patcher temporary files path: ${this.patcherTempPath.absolutePath}" }
        this.patcher = createPatcher()
        logger.info { "Patcher initialized" }
        this.packageMetadata = this.patcher.context.packageMetadata
        logger.info {
            "Package metadata: ${this.packageMetadata.packageName}/${this.packageMetadata.versionName}"
        }

        classList =
            DexReadWrite
                .readMultidexFileFromZip(
                    this.workingFile,
                    this.patcherTempPath.resolve("engine-dex-load"),
                    null,
                ).dexFile
                .classes
                .toList()
        methodList = classList.flatMap { dexClass -> dexClass.methods.toList() }
        classesByType = classList.associateBy { it.type }
        methodsById = methodList.associateBy { it.uniqueId }
        methodsByJavaSignature = buildMethodJavaSignatureIndex(methodList)
        logger.info { "Loaded ${classList.size} classes and ${methodList.size} methods" }
    }

    fun getMethodById(methodId: String): Method? {
        logger.debug { "Searching for method by ID: $methodId" }
        return methodsById[methodId]
    }

    fun getMethodByJavaSignature(javaSignature: String): Method? {
        logger.debug { "Searching for method by Java signature: $javaSignature" }
        return methodsByJavaSignature[javaSignature]
    }

    fun getMethodSmaliStringById(methodId: String): String? {
        logger.debug { "Searching for method string by ID: $methodId" }
        val method = getMethodById(methodId) ?: return null
        val classDef = classesByType[method.definingClass] ?: return null
        return method.toSmaliString(classDef)
    }

    suspend fun runPatchesAndCollectDiffs(patches: List<Patch<*>>): PatchDiffResult {
        logger.debug { "Running ${patches.size} patch(es) for ${sourceFile.name} and collecting diffs" }

        val methodDiffs = mutableMapOf<String, MethodDiff>()
        val classDiffs = mutableListOf<ClassDiff>()
        val originalClassesByType = classesByType
        val snapshotDir = patcherTempPath.resolve("resource_snapshot")
        val resourceTreeDir = patcherTempPath.resolve("apk")

        val comparisonPatch =
            bytecodePatch("\u0000. Comparison patch for diff collection") {
                execute { /* empty — real work happens in finalize */ }
                finalize {
                    collectBytecodeChanges(originalClassesByType, methodDiffs, classDiffs)
                }
            }

        val snapshotPatch =
            rawResourcePatch("\u0000. Snapshot APK resources before patch execution") {
                execute {
                    snapshotDir.deleteRecursively()
                    snapshotDir.parentFile?.mkdirs()
                    if (resourceTreeDir.exists()) {
                        resourceTreeDir.copyRecursively(snapshotDir, overwrite = true)
                    }
                }
            }

        resetPatcher()

        val patchesToRun = patches.toMutableSet()
        patchesToRun += comparisonPatch
        patchesToRun += snapshotPatch
        patcher += patchesToRun
        patcher().collect { result ->
            result.exception?.let { exception ->
                logger.warn(exception) { "Exception during unified patch execution" }
                throwPatchExecutionFailure(
                    phase = "patch execution",
                    patchName = result.patch.name,
                    exception = exception,
                )
            }
        }

        val deletedResourcePaths = patcher.deletedResourcePaths()
        val resourceChanges =
            buildResourceChanges(
                snapshotDir = snapshotDir,
                resourceTreeDir = resourceTreeDir,
                deletedResourcePaths = deletedResourcePaths,
            )

        logger.debug {
            "Finished patch diff. ${methodDiffs.size} method changes, ${classDiffs.size} class changes, " +
                "${resourceChanges.size} resource changes."
        }
        return PatchDiffResult(methodDiffs.values.toList(), classDiffs, resourceChanges)
    }

    /**
     * Runs a bytecode patch and collects all changes: modified, added, and deleted methods,
     * plus class-level structural changes (fields, interfaces, superclass, access flags).
     *
     * Comparison runs in the finalize phase so it captures changes made by both
     * the patch's execute and finalize blocks.
     */
    suspend fun runBytecodePatchesAndDiff(patches: List<BytecodePatch>): BytecodeDiffResult {
        logger.debug { "Running ${patches.size} bytecode patch(es) and diffing methods" }

        val methodDiffs = mutableMapOf<String, MethodDiff>()
        val classDiffs = mutableListOf<ClassDiff>()
        val originalClassesByType = classesByType

        // Named \u0000 so it executes FIRST — its finalize then runs LAST (reverse order),
        // capturing changes from all other patches' execute AND finalize blocks.
        val comparisonPatch =
            bytecodePatch("\u0000. Comparison patch for diff collection") {
                execute { /* empty — real work happens in finalize */ }
                finalize {
                    collectBytecodeChanges(originalClassesByType, methodDiffs, classDiffs)
                }
            }

        resetPatcher()

        patcher.use {
            patcher += (patches.toSet() + comparisonPatch)
            patcher().collect { result ->
                result.exception?.let { exception ->
                    logger.warn(exception) { "Exception during bytecode patch execution in diff iteration" }
                    throwPatchExecutionFailure(
                        phase = "bytecode diff execution",
                        patchName = result.patch.name,
                        exception = exception,
                    )
                }
            }
        }

        logger.debug {
            "Finished bytecode diff. ${methodDiffs.size} method changes, ${classDiffs.size} class changes."
        }
        return BytecodeDiffResult(methodDiffs.values.toList(), classDiffs)
    }

    suspend fun runResourcePatchesAndCompare(patches: List<ResourcePatch>): List<ResourceChange> {
        logger.debug { "Running ${patches.size} resource patch(es) for ${sourceFile.name}" }

        val snapshotDir = patcherTempPath.resolve("resource_snapshot")
        val resourceTreeDir = patcherTempPath.resolve("apk")
        val snapshotPatch =
            resourcePatch("\uFFFF. Snapshot decoded resources before ResourcePatch execution") {
                execute {
                    snapshotDir.deleteRecursively()
                    snapshotDir.parentFile?.mkdirs()
                    if (resourceTreeDir.exists()) {
                        resourceTreeDir.copyRecursively(snapshotDir, overwrite = true)
                    }
                }
            }

        val wrappedPatches =
            patches.map { patchCopy ->
                resourcePatch(patchCopy.name, patchCopy.description, patchCopy.default) {
                    dependsOn(snapshotPatch)
                    dependsOn(*patchCopy.dependencies.toTypedArray())
                    execute { patchCopy.execute(this) }
                    finalize { patchCopy.finalize(this) }
                }
            }

        resetPatcher()

        patcher += wrappedPatches.toSet()
        patcher().collect { result ->
            result.exception?.let { exception ->
                logger.warn(exception) { "Exception during resource patch execution" }
                throwPatchExecutionFailure(
                    phase = "resource execution",
                    patchName = result.patch.name,
                    exception = exception,
                )
            }
        }

        val deletedResourcePaths = patcher.deletedResourcePaths()
        return buildResourceChanges(
            snapshotDir = snapshotDir,
            resourceTreeDir = resourceTreeDir,
            deletedResourcePaths = deletedResourcePaths,
        )
    }

    suspend fun searchFingerprint(fingerprint: Fingerprint): List<Method> {
        logger.debug { "Searching for all methods matching fingerprint: $fingerprint" }
        val matches = mutableListOf<Method>()
        val foundMethodIds = mutableSetOf<String>()

        searchFingerprintSinglePass(fingerprint) { method ->
            if (foundMethodIds.add(method.uniqueId)) {
                matches += method
            }
        }

        logger.debug {
            "Finished searching fingerprint $fingerprint. Found ${foundMethodIds.size} unique methods."
        }
        return matches
    }

    private suspend fun searchFingerprintSinglePass(
        fingerprint: Fingerprint,
        onMatch: (Method) -> Unit,
    ) {
        resetPatcher()
        try {
            val searchPatch =
                bytecodePatch("Temporary patch for exhaustive fingerprint search") {
                    execute {
                        fingerprint.matchAllOrNull()?.forEach { match ->
                            onMatch(match.method)
                        }
                    }
                }

            patcher += setOf(searchPatch)
            patcher().collect { result ->
                result.exception?.let {
                    logger.warn(it) { "Exception during single-pass fingerprint search" }
                }
            }
        } finally {
            fingerprint.clearMatch()
        }
    }

    fun resetPatcher() {
        logger.debug { "Resetting patcher state for ${sourceFile.name}" }
        this.patcher.close()
        this.patcher = createPatcher()
    }

    fun createIsolatedPatcher(temporaryFilesPath: File): Patcher =
        Companion.createPatcher(
            PatcherConfig(
                workingFile,
                temporaryFilesPath,
                null,
                temporaryFilesPath.absolutePath,
            ),
        )

    private fun createPatcher(): Patcher =
        Companion.createPatcher(
            PatcherConfig(
                this.workingFile,
                this.patcherTempPath,
                null,
                this.patcherTempPath.absolutePath,
            ),
        )

    fun dispose() {
        runCatching { patcher.close() }
            .onFailure { logger.warn(it) { "Failed to close patcher for ${sourceFile.absolutePath}" } }
        runCatching { patcherTempPath.deleteRecursively() }
            .onFailure { logger.warn(it) { "Failed to delete patcher temp path ${patcherTempPath.absolutePath}" } }
        temporaryInputPath?.let { tempInputPath ->
            runCatching { tempInputPath.deleteRecursively() }
                .onFailure {
                    logger.warn(it) {
                        "Failed to delete temporary input path ${tempInputPath.absolutePath}"
                    }
                }
        }
    }

    private fun throwPatchExecutionFailure(
        phase: String,
        patchName: String?,
        exception: Throwable,
    ): Nothing {
        val resolvedPatchName = patchName?.takeIf(String::isNotBlank) ?: "patch"
        val reason = exception.message ?: exception::class.qualifiedName ?: "Unknown error"
        throw IllegalStateException(
            "Patch \"$resolvedPatchName\" failed during $phase: $reason",
            exception,
        )
    }

    /**
     * Core bytecode comparison logic, called from within a BytecodePatchContext (finalize block).
     * Iterates all classes to detect:
     * - Modified methods (smali changed)
     * - Added methods (present in mutated class but not original)
     * - Deleted methods (present in original but not mutated class)
     * - New extension classes (no original counterpart)
     * - Class-level structural changes (fields, interfaces, superclass, access flags)
     */
    private fun BytecodePatchContext.collectBytecodeChanges(
        originalClassesByType: Map<String, ClassDef>,
        methodDiffs: MutableMap<String, MethodDiff>,
        classDiffs: MutableList<ClassDiff>,
    ) {
        classDefForEach { classDef ->
            if (classDef !is MutableClass) return@classDefForEach

            val originalClass = originalClassesByType[classDef.type]

            if (originalClass != null) {
                // Existing class that was proxied — compare methods bidirectionally
                val originalMethodsById = originalClass.methods.associateBy { it.uniqueId }
                val mutatedMethodsById = classDef.methods.associateBy { it.uniqueId }

                // Check mutated methods: modified or added
                for (mutatedMethod in classDef.methods) {
                    val originalMethod = originalMethodsById[mutatedMethod.uniqueId]
                    if (originalMethod != null) {
                        val originalSmali = originalMethod.toSmaliString(originalClass)
                        val modifiedSmali = mutatedMethod.toSmaliString(classDef)
                        if (originalSmali != null && modifiedSmali != null && originalSmali != modifiedSmali) {
                            methodDiffs[mutatedMethod.uniqueId] =
                                MethodDiff(
                                    methodId = mutatedMethod.uniqueId,
                                    originalSmali = originalSmali,
                                    modifiedSmali = modifiedSmali,
                                    changeKind = MethodChangeKind.Modified,
                                )
                        }
                    } else {
                        // Method exists in mutated class but not original → Added
                        val modifiedSmali = mutatedMethod.toSmaliString(classDef)
                        if (modifiedSmali != null) {
                            methodDiffs[mutatedMethod.uniqueId] =
                                MethodDiff(
                                    methodId = mutatedMethod.uniqueId,
                                    originalSmali = "",
                                    modifiedSmali = modifiedSmali,
                                    changeKind = MethodChangeKind.Added,
                                )
                        }
                    }
                }

                // Check for deleted methods: present in original but missing in mutated class
                for (originalMethod in originalClass.methods) {
                    if (originalMethod.uniqueId !in mutatedMethodsById) {
                        val originalSmali = originalMethod.toSmaliString(originalClass)
                        if (originalSmali != null) {
                            methodDiffs[originalMethod.uniqueId] =
                                MethodDiff(
                                    methodId = originalMethod.uniqueId,
                                    originalSmali = originalSmali,
                                    modifiedSmali = "",
                                    changeKind = MethodChangeKind.Deleted,
                                )
                        }
                    }
                }

                // Check class-level structural changes (fields, interfaces, superclass, annotations, access flags)
                val originalHeader = originalClass.toHeaderSmaliString()
                val modifiedHeader = classDef.toHeaderSmaliString()
                if (originalHeader != modifiedHeader) {
                    classDiffs.add(
                        ClassDiff(
                            classType = classDef.type,
                            changeKind = ClassChangeKind.Modified,
                            originalHeader = originalHeader,
                            modifiedHeader = modifiedHeader,
                        ),
                    )
                }
            } else {
                // New class (extension or integration) — all methods are new
                for (mutatedMethod in classDef.methods) {
                    val modifiedSmali = mutatedMethod.toSmaliString(classDef)
                    if (modifiedSmali != null) {
                        methodDiffs[mutatedMethod.uniqueId] =
                            MethodDiff(
                                methodId = mutatedMethod.uniqueId,
                                originalSmali = "",
                                modifiedSmali = modifiedSmali,
                                changeKind = MethodChangeKind.Added,
                            )
                    }
                }
                classDiffs.add(
                    ClassDiff(
                        classType = classDef.type,
                        changeKind = ClassChangeKind.Added,
                        originalHeader = "",
                        modifiedHeader = classDef.toHeaderSmaliString(),
                    ),
                )
            }
        }
    }

    private fun buildResourceChanges(
        snapshotDir: File,
        resourceTreeDir: File,
        deletedResourcePaths: Set<String>,
    ): List<ResourceChange> {
        val originalPaths = snapshotDir.collectRelativeFilePaths()
        val modifiedPaths = resourceTreeDir.collectRelativeFilePaths()
        val allPaths = (originalPaths + modifiedPaths + deletedResourcePaths).sorted()

        return buildList {
            allPaths.forEach { relativePath ->
                val originalFile = snapshotDir.resolve(relativePath).takeIf(File::exists)
                val modifiedFile = resourceTreeDir.resolve(relativePath).takeIf(File::exists)
                val isDeleted = relativePath in deletedResourcePaths || (originalFile != null && modifiedFile == null)

                val change =
                    when {
                        isDeleted && originalFile == null -> null
                        isDeleted ->
                            createResourceChange(
                                relativePath = relativePath,
                                kind = ResourceChangeKind.Deleted,
                                originalFile = originalFile,
                                modifiedFile = null,
                            )

                        originalFile == null && modifiedFile != null ->
                            createResourceChange(
                                relativePath = relativePath,
                                kind = ResourceChangeKind.Added,
                                originalFile = null,
                                modifiedFile = modifiedFile,
                            )

                        originalFile != null && modifiedFile != null -> {
                            val originalHash = originalFile.sha256()
                            val modifiedHash = modifiedFile.sha256()
                            if (originalHash == modifiedHash) {
                                null
                            } else {
                                createResourceChange(
                                    relativePath = relativePath,
                                    kind = ResourceChangeKind.Modified,
                                    originalFile = originalFile,
                                    modifiedFile = modifiedFile,
                                    originalHash = originalHash,
                                    modifiedHash = modifiedHash,
                                )
                            }
                        }

                        else -> null
                    }

                if (change != null) {
                    add(change)
                }
            }
        }
    }

    private fun createResourceChange(
        relativePath: String,
        kind: ResourceChangeKind,
        originalFile: File?,
        modifiedFile: File?,
        originalHash: String? = originalFile?.sha256(),
        modifiedHash: String? = modifiedFile?.sha256(),
    ): ResourceChange {
        val isXml = relativePath.lowercase().endsWith(".xml")
        return if (isXml) {
            ResourceChange(
                relativePath = relativePath,
                kind = kind,
                originalContent = originalFile?.readText(Charsets.UTF_8),
                modifiedContent = modifiedFile?.readText(Charsets.UTF_8),
            )
        } else {
            ResourceChange(
                relativePath = relativePath,
                kind = kind,
                originalHash = originalHash,
                modifiedHash = modifiedHash,
            )
        }
    }

    private fun File.collectRelativeFilePaths(): Set<String> {
        if (!exists()) {
            return emptySet()
        }

        return walkTopDown()
            .filter(File::isFile)
            .map { it.relativeTo(this).invariantSeparatorsPath }
            .toSet()
    }

    private fun File.sha256(): String =
        computeContentHash(this)
}

private fun buildMethodJavaSignatureIndex(methods: List<Method>): Map<String, Method> =
    buildMap {
        for (method in methods) {
            val signature = method.javaSignature
            if (signature !in this) {
                put(signature, method)
            }
        }
    }
