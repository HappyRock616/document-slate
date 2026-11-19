# R8 rules for the release build (see build.gradle.kts).
# Rust calls MainActivity / PdfPrintAdapter methods through JNI by name. R8 can't see those
# calls, so without this it would delete or rename them and the app would crash.
-keep class org.documentslate.app.** { *; }
# GameActivity's Java side is called from native code (android-activity) the same way.
-keep class com.google.androidgamesdk.** { *; }
