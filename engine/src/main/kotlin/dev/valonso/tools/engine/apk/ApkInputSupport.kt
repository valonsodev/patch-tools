package dev.valonso.tools.engine.apk

import app.morphe.patcher.apk.ApkMerger
import io.github.oshai.kotlinlogging.KotlinLogging
import java.io.File
import java.util.UUID

internal data class PreparedApkInput(
    val sourceFile: File,
    val workingFile: File,
    val temporaryInputPath: File?,
)

internal object ApkInputSupport {
    private val logger = KotlinLogging.logger("ApkInputSupport")

    private val directApkExtensions = setOf("apk")
    private val mergeableBundleExtensions = setOf("apkm", "xapk")
    private val supportedExtensions = directApkExtensions + mergeableBundleExtensions

    fun supportedInputLabel(): String = ".apk, .apkm, .xapk"

    fun isSupported(file: File): Boolean = file.extension.lowercase() in supportedExtensions

    fun prepare(
        sourceFile: File,
        temporaryRoot: File,
    ): PreparedApkInput {
        val extension = sourceFile.extension.lowercase()
        require(extension in supportedExtensions) {
            "Unsupported Android package input ${sourceFile.name}. Supported formats: ${supportedInputLabel()}"
        }

        if (extension in directApkExtensions) {
            return PreparedApkInput(
                sourceFile = sourceFile,
                workingFile = sourceFile,
                temporaryInputPath = null,
            )
        }

        val temporaryInputPath =
            File(temporaryRoot, "morphe-bundle-${UUID.randomUUID()}")
                .apply { mkdirs() }
        val mergedApk = temporaryInputPath.resolve("${sourceFile.nameWithoutExtension}.apk")

        try {
            logger.info {
                "Preparing bundle input ${sourceFile.absolutePath} via morphe-patcher ApkMerger into ${mergedApk.absolutePath}"
            }
            ApkMerger().merge(sourceFile, mergedApk)
        } catch (exception: Exception) {
            temporaryInputPath.deleteRecursively()
            throw IllegalStateException(
                "Failed to prepare Android package input ${sourceFile.name}: ${exception.message}",
                exception,
            )
        }

        return PreparedApkInput(
            sourceFile = sourceFile,
            workingFile = mergedApk,
            temporaryInputPath = temporaryInputPath,
        )
    }
}
