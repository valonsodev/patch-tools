import com.github.jengelman.gradle.plugins.shadow.tasks.ShadowJar
import org.jetbrains.kotlin.gradle.tasks.KotlinCompile

version = "0.1.0"

plugins {
    `java-library`
    alias(libs.plugins.kotlin.jvm)
    alias(libs.plugins.wire)
    alias(libs.plugins.shadow)
}

kotlin {
    jvmToolchain(17)
    compilerOptions {
        freeCompilerArgs = listOf("-Xcontext-parameters")
    }
}

// Friend-paths: access `internal` members of morphe-patcher
val friends =
    configurations.create("friends") {
        isCanBeResolved = true
        isCanBeConsumed = false
        isTransitive = false
    }
configurations.findByName("implementation")?.extendsFrom(friends)
tasks.withType<KotlinCompile>().configureEach {
    friendPaths.from(friends.incoming.artifactView { }.files)
}

dependencies {
    friends(libs.morphe.patcher)

    implementation(libs.morphe.patcher)
    implementation(libs.morphe.patches.library)
    implementation(libs.morphe.smali)
    implementation(libs.morphe.baksmali)
    implementation(libs.bundles.scripting)
    implementation(libs.kotlin.reflect)
    implementation(libs.kotlinx.coroutines.core)
    implementation(libs.bundles.logging)
    implementation(libs.guava)
    implementation(libs.wire.runtime)
}

wire {
    sourcePath {
        srcDir(rootProject.file("../proto"))
    }
    kotlin {}
}

tasks.named<ShadowJar>("shadowJar") {
    archiveFileName.set("engine-all.jar")
}
