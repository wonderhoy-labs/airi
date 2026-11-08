// What another Android app needs to open Linux apps in Airi, and to find the ones Airi has put on the launcher.
// It's Java with no dependencies, so an app can add the AAR as it is. The README's "Use it in your app" covers it.
plugins {
	id("com.android.library")
}

android {
	namespace = "com.wonderhoylabs.airi.client"
	compileSdk = 37

	defaultConfig {
		// Lower than Airi's own, so an app can use this on any phone and just find that Airi isn't there.
		minSdk = 26
		testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
	}

	compileOptions {
		sourceCompatibility = JavaVersion.VERSION_17
		targetCompatibility = JavaVersion.VERSION_17
	}
}

dependencies {
	androidTestImplementation("androidx.test:runner:1.8.0-alpha03")
	androidTestImplementation("junit:junit:4.13.2")
}
