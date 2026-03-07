load("@aspect_rules_ts//ts:defs.bzl", "ts_project")

ts_project(
    name = "components",
    srcs = glob(["**/*.ts"]),
    declaration = True,
    transpiler = "tsc",
    tsconfig = "//:tsconfig",
    deps = [
        "//:node_modules/lit",
    ],
    visibility = ["//visibility:public"],
)

# Source TS files for the genrule bundle in //server/frontend.
filegroup(
    name = "ts_srcs",
    srcs = glob(["**/*.ts"]),
    visibility = ["//visibility:public"],
)
