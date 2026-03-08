load("@aspect_rules_ts//ts:defs.bzl", "ts_project")
load("@aspect_rules_esbuild//esbuild:defs.bzl", "esbuild")
load("@rules_proto_grpc_js//:defs.bzl", "js_grpc_web_compile")
load("@tar.bzl", "tar")

# ---------------------------------------------------------------------------
# Type-checking (does not produce browser assets).
# ---------------------------------------------------------------------------
ts_project(
    name = "frontend_ts",
    srcs = glob(["src/**/*.ts"]),
    declaration = True,
    transpiler = "tsc",
    tsconfig = "//:tsconfig",
    deps = [
        "//server/components",
        "//:node_modules/lit",
        "//:node_modules/lit-html",
        "//:node_modules/@material/web",
        "//:node_modules/grpc-web",
        "//:node_modules/google-protobuf",
        "//:node_modules/@types/google-protobuf",
    ],
)

# ---------------------------------------------------------------------------
# gRPC-Web generated stubs (CJS, fetched at runtime via fetch()).
# ---------------------------------------------------------------------------
js_grpc_web_compile(
    name = "auth_grpc_web_pb",
    protos = ["//libs/core:api_proto"],
    output_mode = "NO_PREFIX_FLAT",
)

# ---------------------------------------------------------------------------
# Bundle: all TypeScript + npm deps → single browser-ready bundle.js.
# The generated gRPC stubs are NOT bundled; they are fetched at runtime.
# ---------------------------------------------------------------------------
esbuild(
    name = "bundle",
    entry_point = "src/auth-page.ts",
    srcs = glob(["src/**/*.ts"]) + [
        # Component source from the shared package.
        "//server/components:ts_srcs",
        # tsconfig.json is needed for the "friday-components/" path alias.
        "//:tsconfig",
    ],
    deps = [
        "//:node_modules/lit",
        "//:node_modules/@material/web",
        "//:node_modules/grpc-web",
        "//:node_modules/google-protobuf",
    ],
    output = "bundle.js",
    minify = True,
    target = "es2022",
    format = "esm",
)

genrule(
    name = "bundle_js_for_tar",
    srcs = [":bundle"],
    outs = ["bundle_for_tar.js"],
    cmd = "for f in $(locations :bundle); do if [ \"$$(basename $$f)\" = \"bundle.js\" ]; then cp $$f $@; fi; done",
)

# ---------------------------------------------------------------------------
# Extract individual gRPC-Web stub files from the compile output so each
# can be referenced as a single label in the mtree below.
# ---------------------------------------------------------------------------
genrule(
    name = "hello_pb_js",
    srcs = [":auth_grpc_web_pb"],
    outs = ["hello_pb_asset.js"],
    cmd = "for f in $(locations :auth_grpc_web_pb); do if [ \"$$(basename $$f)\" = \"hello_pb.js\" ]; then cp $$f $@; fi; done",
)

genrule(
    name = "hello_grpc_web_pb_js",
    srcs = [":auth_grpc_web_pb"],
    outs = ["hello_grpc_web_pb_asset.js"],
    cmd = "for f in $(locations :auth_grpc_web_pb); do if [ \"$$(basename $$f)\" = \"hello_grpc_web_pb.js\" ]; then cp $$f $@; fi; done",
)

# ---------------------------------------------------------------------------
# Tar archive embedded directly into the Rust binary at compile time via
# include_bytes!(env!("FRONTEND_TAR")).
#
# Contents (relative paths used as URL keys by the in-memory file server):
#   index.html              → /
#   bundle.js               → /bundle.js
#   hello_pb.js             → /hello_pb.js        (fetched at runtime by auth-grpc.ts)
#   hello_grpc_web_pb.js    → /hello_grpc_web_pb.js
#   proto/hello.proto       → /proto/hello.proto
# ---------------------------------------------------------------------------
tar(
    name = "frontend_tar",
    srcs = [
        "index.html",
        ":bundle_js_for_tar",
        ":hello_pb_js",
        ":hello_grpc_web_pb_js",
        "//libs/core:proto/hello.proto",
    ],
    mtree = [
        "index.html uid=0 gid=0 mode=0644 time=1672560000 type=file content=$(location index.html)",
        "bundle.js uid=0 gid=0 mode=0644 time=1672560000 type=file content=$(location :bundle_js_for_tar)",
        "hello_pb.js uid=0 gid=0 mode=0644 time=1672560000 type=file content=$(location :hello_pb_js)",
        "hello_grpc_web_pb.js uid=0 gid=0 mode=0644 time=1672560000 type=file content=$(location :hello_grpc_web_pb_js)",
        "proto uid=0 gid=0 mode=0755 time=1672560000 type=dir",
        "proto/hello.proto uid=0 gid=0 mode=0644 time=1672560000 type=file content=$(location //libs/core:proto/hello.proto)",
    ],
    visibility = ["//visibility:public"],
)
