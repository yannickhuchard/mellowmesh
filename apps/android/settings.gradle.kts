pluginManagement {
    repositories {
        google()
        mavenCentral()
        gradlePluginPortal()
    }
}

dependencyResolutionManagement {
    repositoriesMode.set(RepositoriesMode.FAIL_ON_PROJECT_REPOS)
    repositories {
        google()
        mavenCentral()
    }
}

rootProject.name = "MellowMesh"

// The pure Kotlin core (models, client, pairing, view models) is its own
// standalone build so it compiles and is tested without the Android SDK.
// The app depends on it as "io.mellowmesh:core".
includeBuild("core")

include(":app")
