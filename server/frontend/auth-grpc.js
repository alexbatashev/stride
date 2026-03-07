import * as grpcWebEsm from "https://esm.sh/grpc-web@1.5.0";
import * as jspbEsm from "https://esm.sh/google-protobuf@3.21.4";

let generatedPromise;

function grpcEndpoint() {
  return `${window.location.origin}/grpcweb`;
}

function normalizeGrpcWeb(moduleNs) {
  if (moduleNs?.MethodType) {
    return moduleNs;
  }
  if (moduleNs?.default?.MethodType) {
    return moduleNs.default;
  }
  if (moduleNs?.web?.MethodType) {
    return moduleNs.web;
  }
  if (moduleNs?.grpc?.web?.MethodType) {
    return moduleNs.grpc.web;
  }
  if (moduleNs?.default?.web?.MethodType) {
    return moduleNs.default.web;
  }
  return moduleNs.default ?? moduleNs;
}

function normalizeJspb(moduleNs) {
  return moduleNs.default ?? moduleNs;
}

function patchJspbCompat(jspb) {
  const readerProto = jspb?.BinaryReader?.prototype;
  if (readerProto && typeof readerProto.readStringRequireUtf8 !== "function") {
    readerProto.readStringRequireUtf8 = readerProto.readString;
  }
}

function evalCjsModule(sourceCode, requireFn, sourceName) {
  const module = { exports: {} };
  const factory = new Function(
    "require",
    "module",
    "exports",
    `${sourceCode}\n//# sourceURL=${sourceName}`,
  );
  factory(requireFn, module, module.exports);
  return module.exports;
}

async function loadGenerated() {
  if (!generatedPromise) {
    generatedPromise = (async () => {
      const [pbSource, grpcSource] = await Promise.all([
        fetch("./hello_pb.js").then((r) => r.text()),
        fetch("./hello_grpc_web_pb.js").then((r) => r.text()),
      ]);

      const grpcWeb = normalizeGrpcWeb(grpcWebEsm);
      const jspb = normalizeJspb(jspbEsm);
      patchJspbCompat(jspb);

      const pbModule = evalCjsModule(
        pbSource,
        (id) => {
          if (id === "google-protobuf") return jspb;
          throw new Error(`unsupported require in hello_pb.js: ${id}`);
        },
        "hello_pb.js",
      );

      return evalCjsModule(
        grpcSource,
        (id) => {
          if (id === "grpc-web") return grpcWeb;
          if (id === "./hello_pb.js") return pbModule;
          throw new Error(`unsupported require in hello_grpc_web_pb.js: ${id}`);
        },
        "hello_grpc_web_pb.js",
      );
    })();
  }
  return generatedPromise;
}

export async function register(email, password) {
  const grpc = await loadGenerated();
  const client = new grpc.AuthServicePromiseClient(grpcEndpoint(), null, null);
  const request = new grpc.RegisterRequest();
  request.setEmail(email);
  request.setPassword(password);
  const response = await client.register(request, {});
  return {
    token: response.getToken(),
    user_id: response.getUserId(),
    expires_at: Number(response.getExpiresAt()),
  };
}

export async function login(email, password) {
  const grpc = await loadGenerated();
  const client = new grpc.AuthServicePromiseClient(grpcEndpoint(), null, null);
  const request = new grpc.LoginRequest();
  request.setEmail(email);
  request.setPassword(password);
  const response = await client.login(request, {});
  return {
    token: response.getToken(),
    user_id: response.getUserId(),
    expires_at: Number(response.getExpiresAt()),
  };
}
