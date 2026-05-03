package dev.valonso.tools.engine.scripting

import io.github.oshai.kotlinlogging.KotlinLogging
import kotlinx.coroutines.runBlocking
import java.io.File
import kotlin.script.experimental.annotations.KotlinScript
import kotlin.script.experimental.api.ResultWithDiagnostics
import kotlin.script.experimental.api.ScriptAcceptedLocation
import kotlin.script.experimental.api.ScriptCollectedData
import kotlin.script.experimental.api.ScriptCompilationConfiguration
import kotlin.script.experimental.api.ScriptConfigurationRefinementContext
import kotlin.script.experimental.api.acceptedLocations
import kotlin.script.experimental.api.asSuccess
import kotlin.script.experimental.api.collectedAnnotations
import kotlin.script.experimental.api.compilerOptions
import kotlin.script.experimental.api.defaultImports
import kotlin.script.experimental.api.dependencies
import kotlin.script.experimental.api.ide
import kotlin.script.experimental.api.importScripts
import kotlin.script.experimental.api.isStandalone
import kotlin.script.experimental.api.onSuccess
import kotlin.script.experimental.api.refineConfiguration
import kotlin.script.experimental.api.with
import kotlin.script.experimental.dependencies.CompoundDependenciesResolver
import kotlin.script.experimental.dependencies.DependsOn
import kotlin.script.experimental.dependencies.FileSystemDependenciesResolver
import kotlin.script.experimental.dependencies.Repository
import kotlin.script.experimental.dependencies.maven.MavenDependenciesResolver
import kotlin.script.experimental.dependencies.resolveFromScriptSourceAnnotations
import kotlin.script.experimental.host.FileBasedScriptSource
import kotlin.script.experimental.host.FileScriptSource
import kotlin.script.experimental.jvm.JvmDependency
import kotlin.script.experimental.jvm.dependenciesFromClassloader
import kotlin.script.experimental.jvm.jvm

private val morpheScriptTemplateLogger = KotlinLogging.logger("MorpheScriptTemplate")

@Target(AnnotationTarget.FILE)
@Repeatable
annotation class Import(vararg val paths: String)

@KotlinScript(
    displayName = "Morphe Script",
    fileExtension = "kts",
    compilationConfiguration = MorpheScriptWithMavenDepsConfiguration::class,
)
abstract class MorpheScriptWithMavenDeps

object MorpheScriptWithMavenDepsConfiguration :
    ScriptCompilationConfiguration({
        defaultImports(DependsOn::class, Repository::class, Import::class)
        defaultImports(
            "dev.valonso.tools.engine.scripting.print",
            "dev.valonso.tools.engine.scripting.println",
        )
        jvm {
            // Use java.class.path directly instead of scanning classloaders.
            // In JNI-embedded JVMs the AppClassLoader isn't a URLClassLoader,
            // causing NPE in dependenciesFromClassloader.
            val classPath = System.getProperty("java.class.path")
                ?.split(File.pathSeparator)
                ?.map { File(it) }
                ?.filter { it.exists() }
                ?: emptyList()
            if (classPath.isNotEmpty()) {
                dependencies(JvmDependency(classPath))
            } else {
                dependenciesFromClassloader(
                    wholeClasspath = true,
                    classLoader = MorpheScriptWithMavenDeps::class.java.classLoader,
                )
            }
        }
        ide { acceptedLocations(ScriptAcceptedLocation.Everywhere) }
        isStandalone(true)

        refineConfiguration {
            // Process specified annotations with the provided handler
            onAnnotations(
                DependsOn::class,
                Repository::class,
                Import::class,
                handler = ::configureMavenDepsOnAnnotations,
            )
        }

        compilerOptions.append("-Xjdk-release=11")
        compilerOptions.append("-Xcontext-parameters")
    })

fun configureMavenDepsOnAnnotations(context: ScriptConfigurationRefinementContext): ResultWithDiagnostics<ScriptCompilationConfiguration> {
    val annotations =
        context.collectedData?.get(ScriptCollectedData.collectedAnnotations)?.takeIf {
            it.isNotEmpty()
        } ?: return context.compilationConfiguration.asSuccess()
    val scriptBaseDir = (context.script as? FileBasedScriptSource)?.file?.parentFile
    morpheScriptTemplateLogger.info {
        "Configuring Maven dependencies for script at: ${scriptBaseDir?.path}"
    }
    val importedSources =
        annotations.flatMap {
            (it.annotation as? Import)?.paths?.map { sourceName ->
                FileScriptSource(scriptBaseDir?.resolve(sourceName) ?: File(sourceName))
            } ?: emptyList()
        }
    importedSources.forEach { source ->
        morpheScriptTemplateLogger.info { "Importing script source: ${source.file.path}" }
    }

    // Filter annotations to only pass DependsOn and Repository to the resolver
    // This prevents "Unknown annotation class" errors for Import and CompilerOptions
    val dependencyAnnotations =
        annotations.filter { it.annotation is DependsOn || it.annotation is Repository }

    return runBlocking { resolver.resolveFromScriptSourceAnnotations(dependencyAnnotations) }
        .onSuccess {
            context.compilationConfiguration
                .with {
                    if (importedSources.isNotEmpty()) {
                        importScripts.append(importedSources)
                    }
                    dependencies.append(JvmDependency(it))
                }.asSuccess()
        }
}

private val resolver =
    CompoundDependenciesResolver(FileSystemDependenciesResolver(), MavenDependenciesResolver())
