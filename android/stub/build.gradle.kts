plugins {
    id("com.android.application")
}

// This builds the template for the small apps Airi makes on the phone. Airi swaps in each one's name,
// package and icon. See launcher/AppBuilder.kt.
android {
    namespace = "com.wonderhoylabs.airi.stub"
    compileSdk = 37

    defaultConfig {
        // Airi replaces these two strings in the finished manifest, so they just have to be unique.
        applicationId = "com.wonderhoylabs.airi.linux.template"
        minSdk = 35
        targetSdk = 37
        versionCode = 1
        versionName = "1"
    }

    buildTypes {
        release {
            isMinifyEnabled = false
            // Airi signs each app with this phone's own key, so this signature gets thrown away.
            signingConfig = signingConfigs.getByName("debug")
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    buildFeatures {
        buildConfig = false
    }

    lint {
        // The name is a placeholder that Airi replaces, and nothing here gets translated.
        disable += setOf("HardcodedText", "MissingApplicationIcon")
    }
}

// This is plain Java, and Airi copies it once per Linux app, so it shouldn't carry Kotlin's library.
configurations.matching { it.name.endsWith("RuntimeClasspath") }.configureEach {
    exclude(group = "org.jetbrains.kotlin")
}
