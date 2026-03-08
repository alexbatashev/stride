load("@aspect_rules_ts//ts:defs.bzl", "ts_project")
load("@rules_proto_grpc_js//:defs.bzl", "js_grpc_web_compile")

ts_project(
    name = "frontend_ts",
    srcs = glob(["src/**/*.ts"]),
    declaration = True,
    transpiler = "tsc",
    tsconfig = "//:tsconfig",
    deps = [
        "//server/components",
        "//:node_modules/lit",
        "//:node_modules/@material/web",
    ],
)

js_grpc_web_compile(
    name = "auth_grpc_web_pb",
    protos = ["//libs/core:api_proto"],
    output_mode = "NO_PREFIX_FLAT",
)

# Copy compiled component JS into a components/ subdirectory so that the
# static file server can resolve the "friday-components/" import map prefix.
genrule(
    name = "components_dir",
    srcs = ["//server/components:js_out"],
    outs = ["components/friday-auth-card.js"],
    cmd = """
for f in $(locations //server/components:js_out); do
  name=$$(basename $$f)
  cp $$f $(RULEDIR)/components/$$name
done
""",
)

# Static assets served by the Rust backend at /server/frontend/ as the web root.
#
# URL layout (relative to web root):
#   /                       → index.html (via ServeDir fallback)
#   /src/auth-page.js       → compiled from src/auth-page.ts
#   /src/auth-grpc.js       → compiled from src/auth-grpc.ts
#   /components/*.js        → compiled from //server/components
#   /hello_pb.js            → gRPC-Web protobuf stubs (flat, no prefix)
#   /hello_grpc_web_pb.js   → gRPC-Web client stubs (flat, no prefix)
filegroup(
    name = "frontend",
    srcs = [
        "index.html",
        ":frontend_ts",
        ":auth_grpc_web_pb",
        ":components_dir",
    ],
    visibility = ["//visibility:public"],
)
