// Standalone build: the pure Kotlin/JVM core of the Android app.
// Compiles and tests without the Android SDK:
//   ./gradlew test        (from this directory)
// The Android app includes it as a composite build (see ../settings.gradle.kts).
pluginManagement {
    repositories {
        gradlePluginPortal()
        mavenCentral()
    }
}

dependencyResolutionManagement {
    repositories {
        mavenCentral()
    }
}

rootProject.name = "core"
