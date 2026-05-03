package dev.valonso.tools.engine.session

import dev.valonso.tools.engine.ApkIdentity
import dev.valonso.tools.engine.EngineEvent
import dev.valonso.tools.engine.EngineRunConfig
import dev.valonso.tools.engine.MorpheEngine
import dev.valonso.tools.engine.apk.ApkInputSupport
import dev.valonso.tools.engine.apk.LoadedApk
import dev.valonso.tools.engine.method.uniqueId
import dev.valonso.tools.engine.scripting.MorpheScriptingHost
import io.github.oshai.kotlinlogging.KotlinLogging
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import java.io.File

class EngineSession(
    private val scriptingHost: MorpheScriptingHost,
) : AutoCloseable {
    companion object {
        private val logger = KotlinLogging.logger("EngineSession")
    }

    private val stateMutex = Mutex()

    // -- Internal state --
    private val loadedApks = mutableListOf<LoadedApk>()
    private val apkHashes = mutableSetOf<String>()

    private val _apkIdentities = MutableStateFlow<List<ApkIdentity>>(emptyList())
    val apkIdentities: StateFlow<List<ApkIdentity>> = _apkIdentities.asStateFlow()

    private val _isLoading = MutableStateFlow(false)
    val isLoading: StateFlow<Boolean> = _isLoading.asStateFlow()

    // -- Owned services --
    private val engine = MorpheEngine(scriptingHost)

    // -------------------------------------------------------------------------
    // APK lifecycle
    // -------------------------------------------------------------------------

    suspend fun loadApk(apkFile: File): ApkIdentity? =
        stateMutex.withLock {
            if (!apkFile.exists() || !apkFile.isFile) {
                logger.warn { "Android package file does not exist or is not a file: ${apkFile.absolutePath}" }
                return null
            }

            if (!ApkInputSupport.isSupported(apkFile)) {
                val message =
                    "Unsupported Android package input ${apkFile.absolutePath}. Supported formats: ${ApkInputSupport.supportedInputLabel()}"
                logger.warn { message }
                throw IllegalArgumentException(message)
            }

            _isLoading.value = true
            try {
                val contentHash = withContext(Dispatchers.IO) { LoadedApk.computeContentHash(apkFile) }
                if (!apkHashes.add(contentHash)) {
                    logger.info { "Duplicate APK skipped: ${apkFile.name} (hash=$contentHash)" }
                    return null
                }

                val loadedApk =
                    try {
                        withContext(Dispatchers.IO) {
                            LoadedApk.load(
                                sourceFile = apkFile,
                                patcherTemporaryFilesPath = File(System.getProperty("java.io.tmpdir")),
                                contentHash = contentHash,
                            )
                        }
                    } catch (exception: Exception) {
                        apkHashes.remove(contentHash)
                        throw exception
                    }

                loadedApks.add(loadedApk)
                publishApkIdentities()

                loadedApk.identity
            } finally {
                _isLoading.value = false
            }
        }

    fun unloadApk(apkId: String) {
        runBlocking {
            stateMutex.withLock {
                val index = loadedApks.indexOfFirst { it.id == apkId }
                if (index < 0) return@withLock
                val removed = loadedApks.removeAt(index)
                apkHashes.remove(removed.contentHash)
                removed.dispose()
                publishApkIdentities()
            }
        }
    }

    fun clear() {
        runBlocking {
            stateMutex.withLock {
                clearLocked()
            }
        }
    }

    fun getLoadedApkPaths(): List<String> =
        runBlocking {
            stateMutex.withLock {
                loadedApks.map { it.sourceFile.absolutePath }
            }
        }

    // -------------------------------------------------------------------------
    // Method info
    // -------------------------------------------------------------------------

    fun getMethodSmali(
        apkId: String,
        methodId: String,
    ): String? =
        runBlocking {
            stateMutex.withLock {
                val apk = resolveApkLocked(apkId) ?: return@withLock null
                val resolvedMethodId =
                    apk.getMethodById(methodId)?.let { methodId }
                        ?: apk.getMethodByJavaSignature(methodId)?.uniqueId
                        ?: return@withLock null
                apk.getMethodSmaliStringById(resolvedMethodId)
            }
        }

    fun getMethodInfo(
        apkId: String,
        methodId: String,
    ): MethodInfo? =
        runBlocking {
            stateMutex.withLock {
                val apk = resolveApkLocked(apkId) ?: return@withLock null
                val method = apk.getMethodById(methodId) ?: return@withLock null
                method.toMethodInfo()
            }
        }

    fun methodExists(
        apkId: String,
        methodId: String,
    ): Boolean =
        runBlocking {
            stateMutex.withLock {
                resolveApkLocked(apkId)?.getMethodById(methodId) != null
            }
        }

    fun findExactMethodId(
        apkId: String,
        rawMethodId: String,
    ): String? =
        runBlocking {
            stateMutex.withLock {
                resolveApkLocked(apkId)?.let { apk ->
                    apk.getMethodById(rawMethodId)?.let { rawMethodId }
                        ?: apk.getMethodByJavaSignature(rawMethodId)?.uniqueId
                }
            }
        }

    // -------------------------------------------------------------------------
    // Script execution
    // -------------------------------------------------------------------------

    suspend fun execute(
        scriptPath: String,
        config: EngineRunConfig = EngineRunConfig(),
    ): List<EngineEvent> =
        stateMutex.withLock {
            engine.execute(
                scriptPath,
                loadedApks.toList(),
                config,
            )
        }

    // -------------------------------------------------------------------------
    // Lifecycle
    // -------------------------------------------------------------------------

    override fun close() {
        runBlocking {
            stateMutex.withLock {
                loadedApks.forEach { runCatching { it.dispose() } }
                loadedApks.clear()
                apkHashes.clear()
                _apkIdentities.value = emptyList()
            }
        }
    }

    // -------------------------------------------------------------------------
    // Internal helpers
    // -------------------------------------------------------------------------

    internal suspend fun <T> withResolvedApk(
        apkId: String,
        block: (LoadedApk) -> T,
    ): T? =
        stateMutex.withLock {
            resolveApkLocked(apkId)?.let(block)
        }

    private fun resolveApkLocked(apkId: String): LoadedApk? = loadedApks.find { it.id == apkId }

    private fun publishApkIdentities() {
        _apkIdentities.value = loadedApks.map(LoadedApk::identity)
    }

    private fun clearLocked() {
        loadedApks.forEach { it.dispose() }
        loadedApks.clear()
        apkHashes.clear()
        publishApkIdentities()
    }
}
