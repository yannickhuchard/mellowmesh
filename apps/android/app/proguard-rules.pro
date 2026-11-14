# Release builds are not minified yet (isMinifyEnabled = false).
# When enabling R8, keep kotlinx.serialization serializers of the core models:
-keepattributes *Annotation*, InnerClasses
-keepclassmembers class io.mellowmesh.core.** {
    *** Companion;
    kotlinx.serialization.KSerializer serializer(...);
}
