load("@aspect_rules_ts//ts:defs.bzl", "ts_project")
load("@bazel_lib//lib:copy_to_bin.bzl", "copy_to_bin")

ts_project(
    name = "components",
    srcs = glob(["**/*.ts"]),
    declaration = True,
    transpiler = "tsc",
    tsconfig = "//:tsconfig",
    deps = [
        "//:node_modules/lit",
        "//:node_modules/lit-html",
        "//:node_modules/@material/web",
    ],
    visibility = ["//visibility:public"],
)

# Source TS files copied to bin so cross-package esbuild srcs work.
copy_to_bin(
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
