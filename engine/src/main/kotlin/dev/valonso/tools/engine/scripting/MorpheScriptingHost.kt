package dev.valonso.tools.engine.scripting

import io.github.oshai.kotlinlogging.KotlinLogging
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Deferred
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.async
import java.io.File
import kotlin.script.experimental.api.EvaluationResult
import kotlin.script.experimental.api.ResultValue
import kotlin.script.experimental.api.ResultWithDiagnostics
import kotlin.script.experimental.api.ScriptDiagnostic
import kotlin.script.experimental.api.ScriptEvaluationConfiguration
import kotlin.script.experimental.api.SourceCode
import kotlin.script.experimental.host.FileScriptSource
import kotlin.script.experimental.host.ScriptingHostConfiguration
import kotlin.script.experimental.host.toScriptSource
import kotlin.script.experimental.jvm.baseClassLoader
import kotlin.script.experimental.jvm.jvm
import kotlin.script.experimental.jvmhost.BasicJvmScriptingHost
import kotlin.script.experimental.jvmhost.createJvmCompilationConfigurationFromTemplate
import kotlin.script.experimental.api.CompiledScript
import kotlin.time.Duration.Companion.milliseconds
import kotlin.time.measureTime

sealed class ScriptEvalResult<out T> {
    data class Success<T>(
        val value: T,
        val diagnostics: List<String>,
    ) : ScriptEvalResult<T>()

    data class Failure(
        val errors: List<String>,
        val exception: Throwable? = null,
    ) : ScriptEvalResult<Nothing>()
}

class MorpheScriptingHost {
    private val logger = KotlinLogging.logger("MorpheScriptingHost")

    private val evaluationConfiguration =
        ScriptEvaluationConfiguration {
            jvm { baseClassLoader(MorpheScriptWithMavenDeps::class.java.classLoader) }
        }

    private val scriptingHostConfiguration =
        ScriptingHostConfiguration {
            jvm { baseClassLoader(MorpheScriptWithMavenDeps::class.java.classLoader) }
        }

    private val scriptingHost =
        BasicJvmScriptingHost(baseHostConfiguration = scriptingHostConfiguration)

    /**
     * Cache of compiled scripts keyed by (absolutePath, lastModified).
     * Re-compiles only when the file changes on disk.
     */
    private data class CacheKey(val path: String, val lastModified: Long)
    private val compiledScriptCache = mutableMapOf<CacheKey, CompiledScript>()

    /** Background scope for the preload warm-up. SupervisorJob so a preload failure doesn't cancel callers. */
    private val preloadScope = CoroutineScope(Dispatchers.Default + SupervisorJob())

    /**
     * Warms the Kotlin scripting host by compiling and evaluating `Unit` once.
     * The first real `evaluateScriptFile` awaits this so it can never collide
     * with a preload still in flight.
     */
    private val preload: Deferred<Unit> = preloadScope.async {
        logger.debug { "Preloading BasicJvmScriptingHost..." }
        runCatching {
            val execTime = measureTime { evaluate("Unit".toScriptSource()) }
            logger.debug { "Preloading done in ${execTime.inWholeMilliseconds.milliseconds}" }
        }.onFailure { exception ->
            logger.error(exception) { "Preloading BasicJvmScriptingHost failed" }
        }
        Unit
    }

    private suspend fun compile(script: SourceCode): ResultWithDiagnostics<CompiledScript> =
        scriptingHost.compiler(
            script,
            MorpheScriptWithMavenDepsConfiguration,
        )

    private suspend fun evaluate(script: SourceCode): ResultWithDiagnostics<EvaluationResult> =
        scriptingHost.eval(
            script,
            MorpheScriptWithMavenDepsConfiguration,
            evaluationConfiguration,
        )

    private suspend fun evaluateCompiled(compiled: CompiledScript): ResultWithDiagnostics<EvaluationResult> =
        scriptingHost.evaluator(
            compiled,
            evaluationConfiguration,
        )

    suspend fun evaluateScriptFile(scriptFilePath: String): ScriptEvalResult<Any?> {
        // Block any in-flight preload so the first real evaluation can't race with it.
        preload.await()

        val scriptFile = File(scriptFilePath)
        if (!scriptFile.exists()) {
            return ScriptEvalResult.Failure(errors = listOf("Script file not found: $scriptFilePath"))
        }

        logger.info { "Evaluating script file: $scriptFilePath" }
        var result: ScriptEvalResult<Any?>
        val execTime = measureTime {
            result = try {
                val cacheKey = CacheKey(scriptFile.absolutePath, scriptFile.lastModified())
                val cached = compiledScriptCache[cacheKey]
                if (cached != null) {
                    logger.debug { "Using cached compiled script for $scriptFilePath" }
                    processEvalResultAny(evaluateCompiled(cached))
                } else {
                    logger.debug { "Compiling script: $scriptFilePath" }
                    val script = FileScriptSource(scriptFile)
                    when (val compileResult = compile(script)) {
                        is ResultWithDiagnostics.Success -> {
                            // Evict stale entries for this path
                            compiledScriptCache.keys.removeAll { it.path == scriptFile.absolutePath }
                            compiledScriptCache[cacheKey] = compileResult.value
                            processEvalResultAny(evaluateCompiled(compileResult.value))
                        }
                        is ResultWithDiagnostics.Failure -> {
                            val errors = compileResult.reports.map(::formatDiagnostic)
                            val exception = compileResult.reports.firstOrNull { it.exception != null }?.exception
                            logger.error { "Script compilation failed:\n${errors.joinToString("\n")}" }
                            ScriptEvalResult.Failure(errors, exception)
                        }
                    }
                }
            } catch (exception: Throwable) {
                logger.error(exception) { "Script evaluation threw an unexpected exception" }
                ScriptEvalResult.Failure(
                    errors = buildList {
                        add(exception::class.qualifiedName ?: exception::class.simpleName ?: "Unknown exception")
                        exception.message?.takeIf(String::isNotBlank)?.let(::add)
                    },
                    exception = exception,
                )
            }
        }
        logger.info { "Script evaluation finished in ${execTime.inWholeMilliseconds.milliseconds}ms" }
        return result
    }

    private fun processEvalResultAny(result: ResultWithDiagnostics<EvaluationResult>): ScriptEvalResult<Any?> {
        if (result !is ResultWithDiagnostics.Success) {
            val errors = result.reports.map(::formatDiagnostic)
            val exception = result.reports.firstOrNull { it.exception != null }?.exception
            logger.error { "Script evaluation failed:\n${errors.joinToString("\n")}" }
            exception?.let { logger.error(it) { "  Exception during script evaluation:" } }
            return ScriptEvalResult.Failure(errors, exception)
        }

        val diagnostics = result.reports.map(::formatDiagnostic)

        return when (val returnValue = result.value.returnValue) {
            is ResultValue.Unit -> {
                logger.info { "Script returned Unit." }
                ScriptEvalResult.Success(Unit, diagnostics)
            }

            is ResultValue.Value -> {
                val actualValue = returnValue.value
                if (actualValue == null) {
                    val msg = "Script returned null."
                    logger.warn { msg }
                    ScriptEvalResult.Failure(listOf(msg))
                } else {
                    logger.info {
                        "Returned value type (from script classloader): ${actualValue::class.java.name}"
                    }
                    ScriptEvalResult.Success(actualValue, diagnostics)
                }
            }

            else -> {
                val msg =
                    "Script did not produce a value result. Result type: ${returnValue::class.simpleName}"
                logger.warn { msg }
                ScriptEvalResult.Failure(listOf(msg))
            }
        }
    }

    private fun formatDiagnostic(report: ScriptDiagnostic): String {
        val source = report.sourcePath
        val start = report.location?.start
        val location =
            when {
                source != null && start != null -> "$source:${start.line}:${start.col}"
                source != null -> source
                start != null -> "<script>:${start.line}:${start.col}"
                else -> null
            }
        val prefix =
            if (location != null) {
                "$location: ${report.severity}"
            } else {
                report.severity.toString()
            }

        return "$prefix: ${report.message}"
    }
}
