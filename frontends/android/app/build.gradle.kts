plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

val repositoryRoot = rootProject.projectDir.resolve("../..").canonicalFile
val nativeProject = rootProject.projectDir.resolve("native")
val cargoTargetDir = rootProject.projectDir.resolve("build/cargo-target")
val generatedJniLibs = layout.buildDirectory.dir("generated/e830JniLibs")
val nativeLibrary = layout.buildDirectory.file("generated/e830JniLibs/arm64-v8a/libe830_android_jni.so")
val ndkRoot = System.getenv("ANDROID_NDK_HOME")
    ?: System.getenv("ANDROID_NDK_ROOT")
    ?: System.getenv("ANDROID_HOME")?.let { "$it/ndk/29.0.14206865" }
    ?: System.getenv("ANDROID_SDK_ROOT")?.let { "$it/ndk/29.0.14206865" }

android {
    namespace = "com.samsung.e830"
    compileSdk = 35
    ndkVersion = "29.0.14206865"

    defaultConfig {
        applicationId = "com.samsung.e830"
        minSdk = 24
        targetSdk = 35
        versionCode = 1
        versionName = "0.1.0"
        ndk {
            abiFilters += listOf("arm64-v8a")
        }
    }

    sourceSets["main"].jniLibs.srcDir(generatedJniLibs)

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

}

kotlin {
    compilerOptions {
        jvmTarget.set(org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_17)
    }
}

val buildE830Native by tasks.registering(Exec::class) {
    group = "build"
    description = "Cross-compiles the E830 Unicorn worker for Android ARM64."
    workingDir = repositoryRoot
    val cargo = File(System.getProperty("user.home"), ".cargo/bin/cargo")
    executable = if (cargo.isFile) cargo.absolutePath else "cargo"
    args(
        "ndk",
        "-t", "arm64-v8a",
        "-P", "24",
        "-o", generatedJniLibs.get().asFile.absolutePath,
        "build", "--release", "--jobs", "2",
        "--manifest-path", nativeProject.resolve("Cargo.toml").absolutePath,
    )
    environment("CARGO_TARGET_DIR", cargoTargetDir.absolutePath)
    if (ndkRoot != null) {
        environment("ANDROID_NDK_HOME", ndkRoot)
        environment("ANDROID_NDK_ROOT", ndkRoot)
    }
    inputs.files(
        repositoryRoot.resolve("Cargo.toml"),
        repositoryRoot.resolve("Cargo.lock"),
        repositoryRoot.resolve(".cargo/config.toml"),
        nativeProject.resolve("Cargo.toml"),
        nativeProject.resolve("Cargo.lock"),
        nativeProject.resolve("build.rs"),
    )
    inputs.dir(repositoryRoot.resolve("src"))
    inputs.dir(nativeProject.resolve("src"))
    inputs.file(rootProject.projectDir.resolve("build-support/cmake/android-arm64-v8a.toolchain.cmake"))
    outputs.file(nativeLibrary)
}

tasks.named("preBuild") {
    dependsOn(buildE830Native)
}

dependencies {
    implementation("androidx.core:core-ktx:1.13.1")
    implementation("androidx.appcompat:appcompat:1.7.0")
    implementation("androidx.activity:activity-ktx:1.9.1")
    implementation("androidx.coordinatorlayout:coordinatorlayout:1.2.0")
    implementation("androidx.recyclerview:recyclerview:1.3.2")
    implementation("com.google.android.material:material:1.12.0")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.8.0")
}
