import java.io.File
import org.jetbrains.kotlin.gradle.dsl.JvmTarget

plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.kotlin.android)
    alias(libs.plugins.kotlin.compose)
}

android {
    namespace = "dev.homecloud.android"
    compileSdk = 36
    // Only build-tools 36.0.0 is installed and the SDK directory is read-only,
    // so AGP 8.13.2's default (35.0.0) cannot be provisioned.
    buildToolsVersion = libs.versions.buildTools.get()

    defaultConfig {
        applicationId = "dev.homecloud.android"
        minSdk = 26
        targetSdk = 36
        versionCode = 1
        versionName = "1.0"
    }

    buildFeatures {
        compose = true
    }

    testOptions {
        unitTests.all {
            // Robolectric keeps its runtime-jar cache and download lock under
            // `user.home`. Agent containers can map that to a read-only path, so fall
            // back to the Gradle user home, which is writable and already used for
            // caches (the same reason android/ci/check.sh exports writable homes).
            val userHome = File(System.getProperty("user.home"))
            val writableHome = gradle.gradleUserHomeDir
            if (!userHome.canWrite() && writableHome.canWrite()) {
                it.systemProperty("user.home", writableHome.absolutePath)
            }
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
}

kotlin {
    compilerOptions {
        jvmTarget.set(JvmTarget.JVM_17)
    }
}

dependencies {
    implementation(libs.androidx.core.ktx)
    implementation(libs.androidx.activity.compose)
    implementation(libs.androidx.lifecycle.runtime.ktx)

    implementation(platform(libs.androidx.compose.bom))
    implementation(libs.androidx.compose.ui)
    implementation(libs.androidx.compose.material3)
    implementation(libs.androidx.compose.ui.tooling.preview)
    debugImplementation(libs.androidx.compose.ui.tooling)

    testImplementation(libs.junit)
    testImplementation(libs.robolectric)
}
