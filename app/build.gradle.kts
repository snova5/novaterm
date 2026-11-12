import org.jetbrains.kotlin.gradle.dsl.JvmTarget

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

android {
    namespace = "dev.snova.novaterm"
    compileSdk = 36

    defaultConfig {
        applicationId = "dev.snova.novaterm"
        minSdk = 30
        targetSdk = 36          // Android 16; runs on Android 17 as well
        versionCode = 1
        versionName = "0.1.0"
        ndk { abiFilters += listOf("arm64-v8a", "x86_64") }
    }

    buildTypes {
        release {
            isMinifyEnabled = false
            // Signed with the debug key so `assembleRelease` gives an installable APK.
            // Replace with your own keystore before publishing.
            signingConfig = signingConfigs.getByName("debug")
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
}

kotlin {
    compilerOptions { jvmTarget.set(JvmTarget.JVM_17) }
}

// Build the Rust core for Android with cargo-ndk and drop the .so files
// where the Android Gradle Plugin picks them up.
val cargoNdk = tasks.register<Exec>("cargoNdk") {
    val rustDir = rootProject.file("rust")
    val outDir = file("src/main/jniLibs")
    workingDir = rustDir
    commandLine(
        "cargo", "ndk",
        "-t", "arm64-v8a", "-t", "x86_64",
        "--platform", "30",
        "-o", outDir.absolutePath,
        "build", "--release",
    )
    inputs.dir(rustDir.resolve("src"))
    inputs.file(rustDir.resolve("Cargo.toml"))
    outputs.dir(outDir)
}

tasks.named("preBuild") { dependsOn(cargoNdk) }
