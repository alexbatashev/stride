import org.jetbrains.compose.desktop.application.dsl.TargetFormat
import org.jetbrains.kotlin.gradle.dsl.JvmTarget

plugins {
    alias(libs.plugins.kotlinMultiplatform)
    alias(libs.plugins.androidApplication)
    alias(libs.plugins.composeMultiplatform)
    alias(libs.plugins.composeCompiler)
    alias(libs.plugins.composeHotReload)
}

kotlin {
    androidTarget {
        compilerOptions {
            jvmTarget.set(JvmTarget.JVM_11)
        }
    }

    jvm()

    sourceSets {
        androidMain.dependencies {
            implementation(libs.compose.uiToolingPreview)
            implementation(libs.androidx.activity.compose)
        }
        commonMain.dependencies {
            implementation(libs.compose.runtime)
            implementation(libs.compose.foundation)
            implementation(libs.compose.material3)
            implementation(libs.compose.ui)
            implementation(libs.compose.components.resources)
            implementation(libs.compose.uiToolingPreview)
            implementation(libs.androidx.lifecycle.viewmodelCompose)
            implementation(libs.androidx.lifecycle.runtimeCompose)
        }
        commonTest.dependencies {
            implementation(libs.kotlin.test)
        }
        jvmMain.dependencies {
            implementation(compose.desktop.currentOs)
            implementation(libs.kotlinx.coroutinesSwing)
        }
    }
}

android {
    namespace = "me.batashev.friday"
    compileSdk = libs.versions.android.compileSdk.get().toInt()

    defaultConfig {
        applicationId = "me.batashev.friday"
        minSdk = libs.versions.android.minSdk.get().toInt()
        targetSdk = libs.versions.android.targetSdk.get().toInt()
        versionCode = 1
        versionName = "1.0"
    }
    packaging {
        resources {
            excludes += "/META-INF/{AL2.0,LGPL2.1}"
        }
    }
    buildTypes {
        getByName("release") {
            isMinifyEnabled = false
        }
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_11
        targetCompatibility = JavaVersion.VERSION_11
    }
}

dependencies {
    debugImplementation(libs.compose.uiTooling)
}

compose.desktop {
    application {
        mainClass = "me.batashev.friday.MainKt"

        nativeDistributions {
            targetFormats(TargetFormat.Dmg, TargetFormat.Msi, TargetFormat.Deb)
            packageName = "me.batashev.friday"
            packageVersion = "1.0.0"
        }
    }
}

val composeRootDir = rootProject.projectDir
val repoRootDir = composeRootDir.parentFile
val scriptsDir = File(composeRootDir, "scripts")
val autoBridgeEnabled = providers.gradleProperty("friday.swiftBridge.autoBuild")
    .orNull
    ?.toBoolean()
    ?: true
val strictBridgeBuild = providers.gradleProperty("friday.swiftBridge.strict")
    .orNull
    ?.toBoolean()
    ?: false

fun Exec.configureBridgeEnvironment() {
    if (!autoBridgeEnabled) {
        enabled = false
        return
    }
    isIgnoreExitValue = !strictBridgeBuild
}

val buildSwiftBridgeAndroid by tasks.registering(Exec::class) {
    group = "interop"
    description = "Build Swift Friday bridge for Android ABIs."
    workingDir = repoRootDir
    commandLine("bash", File(scriptsDir, "build-swift-bridge.sh").absolutePath, "android")
    configureBridgeEnvironment()
}

val buildJniBridgeAndroid by tasks.registering(Exec::class) {
    group = "interop"
    description = "Build JNI bridge for Android ABIs."
    workingDir = repoRootDir
    commandLine("bash", File(scriptsDir, "build-jni-bridge.sh").absolutePath, "android")
    configureBridgeEnvironment()
    dependsOn(buildSwiftBridgeAndroid)
}

val buildSwiftBridgeLinux by tasks.registering(Exec::class) {
    group = "interop"
    description = "Build Swift Friday bridge for Linux host."
    workingDir = repoRootDir
    commandLine("bash", File(scriptsDir, "build-swift-bridge.sh").absolutePath, "linux")
    configureBridgeEnvironment()
}

val buildJniBridgeLinux by tasks.registering(Exec::class) {
    group = "interop"
    description = "Build JNI bridge for Linux host."
    workingDir = repoRootDir
    commandLine("bash", File(scriptsDir, "build-jni-bridge.sh").absolutePath, "linux")
    configureBridgeEnvironment()
    dependsOn(buildSwiftBridgeLinux)
}

tasks.named("preBuild") {
    dependsOn(buildJniBridgeAndroid)
}

tasks.matching { it.name == "run" || it.name == "runDistributable" }.configureEach {
    dependsOn(buildJniBridgeLinux)
}
