package(default_visibility = ["//visibility:public"])

COMPOSE_INPUTS = glob(
    [
        "build.gradle.kts",
        "settings.gradle.kts",
        "gradle.properties",
        "gradle/libs.versions.toml",
        "gradlew",
        "gradlew.bat",
        "composeApp/build.gradle.kts",
        "composeApp/src/**",
    ],
)

genrule(
    name = "desktop_app",
    srcs = COMPOSE_INPUTS,
    tools = [
        "//libs/core:friday_cdylib",
        "//libs/core:kotlin_bindings",
    ],
    outs = ["desktop_app.ok"],
    cmd = """
set -eu
OUT="$@"
cd compose
./gradlew --no-daemon :composeApp:compileKotlinJvm \
  -Pfriday.core.kotlinBindingsFile=$(location //libs/core:kotlin_bindings) \
  -Pfriday.core.desktopLibFile=$(location //libs/core:friday_cdylib)
echo desktop_ok > "$OUT"
""",
)

genrule(
    name = "android_app",
    srcs = COMPOSE_INPUTS,
    tools = [
        "//libs/core:friday_cdylib",
        "//libs/core:kotlin_bindings",
    ],
    outs = ["android_app.ok"],
    cmd = """
set -eu
OUT="$@"
cd compose
./gradlew --no-daemon :composeApp:assembleDebug \
  -Pfriday.core.kotlinBindingsFile=$(location //libs/core:kotlin_bindings) \
  -Pfriday.core.desktopLibFile=$(location //libs/core:friday_cdylib)
echo android_ok > "$OUT"
""",
)
