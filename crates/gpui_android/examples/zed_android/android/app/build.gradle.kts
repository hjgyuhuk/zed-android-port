import java.util.Properties

plugins {
    id("com.android.application")
    kotlin("android")
}

// Release signing config. `signing.properties` and `release.keystore` are
// gitignored: contributors who clone the repo can still build the debug
// variant, but a release build that produces a signed APK requires the
// keystore + properties to be present locally (or provided by CI via
// a separate signing job in CI).
val signingPropsFile = file("signing.properties")
val signingProps = Properties().apply {
    if (signingPropsFile.exists()) {
        signingPropsFile.inputStream().use { load(it) }
    }
}
val hasReleaseSigning = signingPropsFile.exists()

val workspaceRoot = file("../../../../../..").canonicalFile

// zed-askpass-helper bundling.
//
// `zed-askpass-helper` is the SSH_ASKPASS relay binary that the system's
// ssh exec's to prompt the user for a password via Zed's in-process
// modal. It also stands in for `zed --printenv` in Zed's shell capture.
// It ships as `libaskpass.so` in jniLibs so the installer extracts it to
// ApplicationInfo.nativeLibraryDir, the one app-owned location an
// untrusted app may exec from at any targetSdk (W^X forbids exec'ing
// anything from the app's writable dirs).
//
// The helper crate at `examples/zed_android/askpass-helper/` is its
// own workspace and ships a `.cargo/config.toml` setting
// `target-feature=+crt-static` for `aarch64-linux-android`. By running
// `cargo ndk` with `workingDir` set to that crate's directory, gradle
// guarantees the static-link config is in scope for every APK build
// — no chance of a contributor accidentally shipping a dynamic
// binary by building from the wrong cwd or hand-editing the artifact.
val askpassHelperDir = file("${workspaceRoot}/crates/gpui_android/examples/zed_android/askpass-helper")
val askpassHelperBin = file("${askpassHelperDir}/target/aarch64-linux-android/release/zed-askpass-helper")
val askpassHelperJniLib = file("src/main/jniLibs/arm64-v8a/libaskpass.so")

tasks.register<Exec>("buildAskpassHelper") {
    description = "Build zed-askpass-helper (static aarch64) via cargo-ndk."
    group = "build setup"

    workingDir(askpassHelperDir)
    commandLine(
        "cargo",
        "ndk",
        "-t",
        "arm64-v8a",
        "-P",
        "31",
        "build",
        "--release",
        "--locked",
        "--target-dir",
        file("${askpassHelperDir}/target").absolutePath,
    )

    providers.environmentVariable("ANDROID_NDK_HOME").orNull?.let { ndk ->
        environment("ANDROID_NDK_HOME", ndk)
    }
    // Cargo lets an environment RUSTFLAGS replace the target rustflags in
    // the crate's .cargo/config.toml, which for the helper means losing the
    // static link. The helper's own config governs whatever the environment
    // holds (a device-specific build may export RUSTFLAGS for the app).
    environment.remove("RUSTFLAGS")
    environment.remove("CARGO_ENCODED_RUSTFLAGS")

    outputs.upToDateWhen { false }
    outputs.file(askpassHelperBin)
}

tasks.register<Copy>("stageAskpassHelperJniLib") {
    description = "Copy the freshly-built zed-askpass-helper into jniLibs as libaskpass.so."
    group = "build setup"

    dependsOn("buildAskpassHelper")
    from(askpassHelperBin)
    into(askpassHelperJniLib.parentFile)
    rename { askpassHelperJniLib.name }
    inputs.file(askpassHelperBin)
    outputs.file(askpassHelperJniLib)
}

tasks.matching { it.name == "preBuild" }.configureEach {
    dependsOn("stageAskpassHelperJniLib")
}

android {
    namespace = "com.zdroid"
    compileSdk = 35

    // Pinned so builds don't depend on which NDK happens to be installed.
    ndkVersion = "27.0.12077973"

    defaultConfig {
        applicationId = "com.zdroid"
        // Android 12. Raised from 26 for 0.8.5's Armv9 build and kept when
        // 0.15.0 went back to Armv8.2 cores (.cargo/config.toml), since
        // nothing since has been checked below it. Keep in sync with `-P`
        // in the task above.
        minSdk = 31
        // targetSdk 35. What the level buys: no "built for an older
        // version" install warning, a real notification permission prompt
        // (33+), and the foreground service type declared here is enforced
        // (34+). What it costs: raw /sdcard paths need All files access
        // (MANAGE_EXTERNAL_STORAGE); see AndroidManifest.xml. Exec'ing
        // app-owned files is only allowed from the native library dir,
        // which is where the askpass helper ships.
        targetSdk = 35
        versionCode = 22
        versionName = "0.4.4"
        ndk {
            abiFilters += listOf("arm64-v8a")
        }
    }

    sourceSets {
        getByName("main") {
            jniLibs.srcDirs("src/main/jniLibs")
        }
    }

    // The askpass helper must exist as a real file on disk: the kernel
    // execs it by path from ApplicationInfo.nativeLibraryDir. AGP 8 keeps
    // native libs uncompressed inside the APK by default (extractNativeLibs
    // = false), so opt back into extraction; this DSL is what writes the
    // manifest attribute. keepDebugSymbols keeps the strip task off the
    // statically linked helper ELF.
    packaging {
        jniLibs {
            useLegacyPackaging = true
            keepDebugSymbols += "**/libaskpass.so"
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    kotlinOptions {
        jvmTarget = "17"
    }

    signingConfigs {
        if (hasReleaseSigning) {
            create("release") {
                storeFile = file(signingProps.getProperty("storeFile"))
                storePassword = signingProps.getProperty("storePassword")
                keyAlias = signingProps.getProperty("keyAlias")
                keyPassword = signingProps.getProperty("keyPassword")
            }
        }
    }

    buildTypes {
        getByName("debug") {
            isMinifyEnabled = false
        }
        getByName("release") {
            isMinifyEnabled = false
            // Strip the native libraries in the APK, but keep their symbol
            // tables in build/outputs/native-debug-symbols for symbolizing
            // profiles and crashes. CI publishes the file with each build.
            ndk {
                debugSymbolLevel = "SYMBOL_TABLE"
            }
            if (hasReleaseSigning) {
                signingConfig = signingConfigs.getByName("release")
            }
        }
    }
}

dependencies {
    // android-activity's native GameActivity code is version 4.4 and
    // supports the 4.0 Java side. 4.4.0's Java side attaches input listeners
    // to its SurfaceView, which send captured mouse and trackpad events
    // through native code that leaks them, and it focuses that SurfaceView
    // on every window focus change, away from the IME host.
    implementation("androidx.games:games-activity:4.0.0")
    implementation("androidx.appcompat:appcompat:1.7.0")
    implementation("androidx.core:core-ktx:1.13.1")
}