plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.plugin.compose")
}

// VERSION at the top of the repo is shared with the agent, and the app upgrades an agent that differs.
val appVersion = rootProject.file("../VERSION").readText().trim()
val appVersionCode = appVersion.split(".").map(String::toInt).let { (major, minor, patch) ->
    major * 10_000 + minor * 100 + patch
}

android {
    namespace = "com.wonderhoylabs.airi"
    compileSdk = 37
    ndkVersion = "30.0.16248370"

    defaultConfig {
        applicationId = "com.wonderhoylabs.airi"
        // Android's Linux terminal arrived in Android 15.
        minSdk = 35
        targetSdk = 37
        versionCode = appVersionCode
        versionName = appVersion
        // Android's Linux terminal only exists on 64-bit Arm.
        ndk { abiFilters += "arm64-v8a" }
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
    }

    // Set these to sign with your own key. Without them a release APK still installs, because it
    // falls back to the debug key, but Android refuses to update an app across different keys.
    signingConfigs {
        System.getenv("AIRI_KEYSTORE")?.let { keystore ->
            create("release") {
                storeFile = file(keystore)
                storePassword = System.getenv("AIRI_KEYSTORE_PASSWORD")
                keyAlias = System.getenv("AIRI_KEY_ALIAS")
                keyPassword = System.getenv("AIRI_KEY_PASSWORD")
            }
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = true
            isShrinkResources = true
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"), "proguard-rules.pro")
            signingConfig = signingConfigs.findByName("release") ?: signingConfigs.getByName("debug")
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    buildFeatures {
        buildConfig = true
        compose = true
    }

    testOptions {
        unitTests.isReturnDefaultValues = true
    }

    packaging {
        // apksig carries a notice file that nothing reads at runtime.
        resources.excludes += "META-INF/**"
    }

    sourceSets["main"].jniLibs.directories.add(layout.buildDirectory.dir("rust/jniLibs").get().asFile.path)
    sourceSets["main"].assets.directories.add(layout.buildDirectory.dir("generated/bundled").get().asFile.path)
}

// The viewer's native half, which decompresses frames and draws them.
val buildRust = tasks.register<Exec>("buildRust") {
    val output = layout.buildDirectory.dir("rust/jniLibs")
    val ndk = androidComponents.sdkComponents.ndkDirectory
    workingDir = rootProject.projectDir.parentFile
    inputs.dir(rootProject.file("../crates/airi-viewer"))
    inputs.file(rootProject.file("../Cargo.lock"))
    outputs.dir(output)
    doFirst { environment("ANDROID_NDK_HOME", ndk.get().asFile.path) }
    commandLine(
        "cargo", "ndk", "--platform", "35", "-o", output.get().asFile.path, "-t", "arm64-v8a",
        "build", "--release", "-p", "airi-viewer",
    )
}

// The agent has to be built on 64-bit Arm Linux, which `tools/build-agent.sh` and CI do. Without one a
// debug build still works against an agent that's already installed, but it can't install a new one.
val agentBinary = System.getenv("AIRI_AGENT_BINARY")?.let(::file) ?: rootProject.file("../dist/airi-agent")

val bundleFiles = tasks.register<Copy>("bundleFiles") {
    from(project(":stub").layout.buildDirectory.file("outputs/apk/release/stub-release.apk")) {
        rename { "app-template.apk" }
    }
    if (agentBinary.exists()) from(agentBinary)
    into(layout.buildDirectory.dir("generated/bundled"))
    dependsOn(":stub:assembleRelease")
}

tasks.configureEach {
    if (name.startsWith("merge") && name.endsWith("JniLibFolders")) dependsOn(buildRust)
    if (name.startsWith("merge") && name.endsWith("Assets")) dependsOn(bundleFiles)
    if (name.startsWith("lint") || name.contains("LintVitalReport") || name.contains("LintReport")) {
        mustRunAfter(bundleFiles)
    }
    if (name == "assembleRelease" || name == "bundleRelease") {
        doFirst {
            check(agentBinary.exists()) { "Build the agent first: there's nothing at $agentBinary" }
        }
    }
}

dependencies {
    implementation(platform("androidx.compose:compose-bom:2026.09.00"))
    implementation("androidx.activity:activity-compose:1.13.0")
    implementation("androidx.compose.foundation:foundation")
    implementation("androidx.compose.material3:material3")
    implementation("androidx.compose.ui:ui")
    implementation("androidx.core:core-ktx:1.19.1")
    implementation("androidx.lifecycle:lifecycle-runtime-compose:2.11.0")
    // Signs the apps Airi makes. This is Android's own signing library.
    implementation("com.android.tools.build:apksig:9.5.1")

    testImplementation("junit:junit:4.13.2")

    androidTestImplementation(platform("androidx.compose:compose-bom:2026.09.00"))
    androidTestImplementation("androidx.compose.ui:ui-test-junit4")
    // Compose's test library brings an older one, which can't run on recent versions of Android.
    androidTestImplementation("androidx.test.espresso:espresso-core:3.8.0-alpha03")
    androidTestImplementation("androidx.test:runner:1.8.0-alpha03")
    debugImplementation("androidx.compose.ui:ui-test-manifest")
}
