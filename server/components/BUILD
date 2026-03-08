load("@aspect_rules_ts//ts:defs.bzl", "ts_project")

ts_project(
    name = "components",
    srcs = glob(["**/*.ts"]),
    declaration = True,
    transpiler = "tsc",
    tsconfig = "//:tsconfig",
    deps = [
        "//:node_modules/lit",
        "//:node_modules/@material/web",
    ],
    visibility = ["//visibility:public"],
)

# Source TS files for type-checking by downstream packages.
filegroup(
    name = "ts_srcs",
    srcs = glob(["**/*.ts"]),
    visibility = ["//visibility:public"],
)

# Compiled JS outputs for serving under /components/.
filegroup(
    name = "js_out",
    srcs = [":components"],
    visibility = ["//visibility:public"],
)
