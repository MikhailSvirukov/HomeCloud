import org.jetbrains.kotlin.gradle.dsl.JvmTarget

plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.kotlin.android)
    alias(libs.plugins.kotlin.compose)
    alias(libs.plugins.ktlint)
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
        // Compose/Robolectric render tests read the merged debug manifest and resources; without
        // this Robolectric only sees the framework and cannot resolve the test activity that
        // ui-test-manifest declares.
        unitTests.isIncludeAndroidResources = true
        unitTests.all {
            // Robolectric keeps its runtime-jar cache and download lock under
            // `user.home`, which agent containers can map to a read-only path. Always
            // point the forked test JVM at the Gradle user home: it is writable, it is
            // where Gradle already keeps caches, and the CI workflow caches it, so a
            // runner stops re-downloading the jars on every change.
            it.systemProperty("user.home", gradle.gradleUserHomeDir.absolutePath)
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

ktlint {
    // Compose mandates PascalCase composable names; the naming rule still applies elsewhere.
    additionalEditorconfig.put("ktlint_function_naming_ignore_when_annotated_with", "Composable")
}

dependencies {
    implementation(libs.androidx.core.ktx)
    implementation("com.github.luben:zstd-jni:${libs.versions.zstdJni.get()}@aar")
    implementation(libs.androidx.exifinterface)
    implementation(libs.androidx.activity.compose)
    implementation(libs.androidx.lifecycle.runtime.ktx)

    implementation(platform(libs.androidx.compose.bom))
    implementation(libs.androidx.compose.ui)
    implementation(libs.androidx.compose.material3)
    implementation(libs.androidx.compose.ui.tooling.preview)
    debugImplementation(libs.androidx.compose.ui.tooling)

    testImplementation(platform(libs.androidx.compose.bom))
    testImplementation(libs.androidx.compose.ui.test.junit4)
    debugImplementation(libs.androidx.compose.ui.test.manifest)

    testImplementation(libs.junit)
    testImplementation(libs.robolectric)
    testImplementation(libs.zstd.jni)
}
