# The native library finds these by name.
-keep class com.wonderhoylabs.airi.viewer.NativeView { native <methods>; }
# The signing library looks up parts of itself by name.
-keep class com.android.apksig.** { *; }
-dontwarn com.android.apksig.**
