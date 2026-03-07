load("@aspect_rules_ts//ts:defs.bzl", "ts_project")
load("@rules_proto_grpc_js//:defs.bzl", "js_grpc_web_compile")

# Type-checking only.
ts_project(
    name = "frontend_ts",
    srcs = glob(["**/*.ts"]),
    declaration = True,
    transpiler = "tsc",
    tsconfig = "//:tsconfig",
    deps = [
        "//server/components",
        "//:node_modules/lit",
    ],
)

js_grpc_web_compile(
    name = "auth_grpc_web_pb",
    protos = ["//libs/core:api_proto"],
    output_mode = "NO_PREFIX_FLAT",
)

# Static assets served by the Rust backend.
filegroup(
    name = "frontend",
    srcs = glob(["*.html", "*.js", "*.css"]) + [":auth_grpc_web_pb"],
    visibility = ["//visibility:public"],
)
