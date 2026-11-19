import java.util.Properties

plugins {
    id("com.android.application")
}

// Play Store upload key. Its location and passwords live in android/keystore.properties, which is
// git-ignored (as are *.jks / *.keystore) and must never be committed. Keys in that file:
//   storeFile=C:/path/outside/the/repo/upload-key.jks
//   storePassword=...
//   keyAlias=upload
//   keyPassword=...
// Without the file, release builds fall back to the debug key (fine for sideloading only).
val uploadKey = rootProject.file("keystore.properties").takeIf { it.exists() }?.let { f ->
    Properties().apply { f.inputStream().use { load(it) } }
}

android {
    namespace = "org.documentslate.app"
    compileSdk = 37
    ndkVersion = "30.0.16248370"

    defaultConfig {
        applicationId = "org.documentslate.app"
        minSdk = 26
        targetSdk = 37
        versionCode = 1
        versionName = "0.1.0"
    }

    signingConfigs {
        if (uploadKey != null) {
            create("upload") {
                storeFile = file(uploadKey.getProperty("storeFile"))
                storePassword = uploadKey.getProperty("storePassword")
                keyAlias = uploadKey.getProperty("keyAlias")
                keyPassword = uploadKey.getProperty("keyPassword")
            }
        }
    }

    buildTypes {
        release {
            // Phones only: arm64-v8a covers every 64-bit Android phone/tablet from the last
            // ~8 years. The x86_64 emulator library stays in the debug APK only.
            ndk { abiFilters += listOf("arm64-v8a") }
            // R8 shrinks the small Java part (and the androidx libraries it pulls in) and drops
            // unused resources. proguard-rules.pro keeps the Java methods Rust calls by name.
            isMinifyEnabled = true
            isShrinkResources = true
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"), "proguard-rules.pro")
            // Upload key if android/keystore.properties exists, else the debug key (sideload only).
            // `-Punsigned` (used by F-Droid's build server) leaves the APK unsigned so F-Droid can
            // sign it with its own key.
            signingConfig = when {
                project.hasProperty("unsigned") -> null
                uploadKey != null -> signingConfigs.getByName("upload")
                else -> signingConfigs.getByName("debug")
            }
        }
    }

    // The Rust .so files are built by cargo-ndk into src/main/jniLibs
    // (see build-android.ps1); Gradle just packages them.
    // Release uses the size-optimised library that build-release.ps1 puts in
    // android/release-jniLibs/ instead (a build-type folder wins over src/main).
    sourceSets {
        getByName("release") {
            jniLibs.directories.add("../release-jniLibs")
        }
    }

    // Store the .so compressed inside the APK: much smaller APK file to copy/download.
    // Android unpacks it at install time, so installed size = APK + unpacked .so.
    packaging {
        jniLibs {
            useLegacyPackaging = true
            // cargo-ndk also copies the .so files some dependencies build for themselves (their
            // code is already inside libdocument_slate.so); the app never loads them.
            excludes += setOf("**/libboa_engine.so", "**/libocrs.so", "**/librten.so")
        }
    }

    // Don't embed Google's encrypted dependency report (readable only by Google Play).
    // F-Droid's scanner rejects it; Play doesn't need it.
    dependenciesInfo {
        includeInApk = false
        includeInBundle = false
    }
}

dependencies {
    // GameActivity version must match what android-activity 0.6 expects (its README).
    implementation("androidx.games:games-activity:4.4.0")
    implementation("androidx.appcompat:appcompat:1.7.1")
}
