/**
 * gRPC-Web client for the AuthService.
 *
 * grpc-web and google-protobuf are bundled by esbuild (no CDN needed).
 *
 * The Bazel-generated protobuf stubs (hello_pb.js, hello_grpc_web_pb.js) are
 * CommonJS modules that cannot be statically imported from an ES module
 * without a bundler. They are NOT bundled — they ship as separate files in
 * frontend.tar and are fetched at runtime from the same origin.
 */

// Bundled by esbuild from node_modules.
import * as grpcWebEsm from 'grpc-web';
import * as jspbEsm from 'google-protobuf';

// ---------------------------------------------------------------------------
// Types for the CJS modules loaded at runtime
// ---------------------------------------------------------------------------

// eslint-disable-next-line @typescript-eslint/no-explicit-any
type AnyNs = any;

interface AuthServiceClient {
  register(req: RegisterRequestMsg, meta: Record<string, string>): Promise<AuthReplyMsg>;
  login(req: LoginRequestMsg, meta: Record<string, string>): Promise<AuthReplyMsg>;
  logout(req: LogoutRequestMsg, meta: Record<string, string>): Promise<LogoutReplyMsg>;
}

interface RegisterRequestMsg {
  setEmail(v: string): void;
  setPassword(v: string): void;
}

interface LoginRequestMsg {
  setEmail(v: string): void;
  setPassword(v: string): void;
}

interface AuthReplyMsg {
  getToken(): string;
  getUserId(): string;
  getExpiresAt(): number | bigint;
}

interface LogoutRequestMsg {
  setToken(v: string): void;
}

interface LogoutReplyMsg {
  getSuccess(): boolean;
}

interface GeneratedModule {
  AuthServicePromiseClient: new (
    endpoint: string,
    credentials: null,
    options: unknown,
  ) => AuthServiceClient;
  RegisterRequest: new () => RegisterRequestMsg;
  LoginRequest: new () => LoginRequestMsg;
  LogoutRequest: new () => LogoutRequestMsg;
}

// ---------------------------------------------------------------------------
// CJS loader
// ---------------------------------------------------------------------------

let generatedPromise: Promise<GeneratedModule> | null = null;

function grpcEndpoint(): string {
  const w = window as Window & { FRIDAY_GRPC_ENDPOINT?: string };
  if (w.FRIDAY_GRPC_ENDPOINT) return w.FRIDAY_GRPC_ENDPOINT;
  return window.location.origin;
}

function normalizeGrpcWeb(ns: AnyNs): AnyNs {
  if (ns?.MethodType) return ns;
  if (ns?.default?.MethodType) return ns.default;
  if (ns?.web?.MethodType) return ns.web;
  if (ns?.grpc?.web?.MethodType) return ns.grpc.web;
  if (ns?.default?.web?.MethodType) return ns.default.web;
  return ns.default ?? ns;
}

function normalizeJspb(ns: AnyNs): AnyNs {
  return ns.default ?? ns;
}

function patchJspbCompat(jspbLib: AnyNs): void {
  const proto = jspbLib?.BinaryReader?.prototype;
  if (proto && typeof proto.readStringRequireUtf8 !== 'function') {
    proto.readStringRequireUtf8 = proto.readString;
  }
}

function evalCjsModule(
  source: string,
  requireFn: (id: string) => AnyNs,
  name: string,
): AnyNs {
  const mod = { exports: {} as AnyNs };
  // eslint-disable-next-line no-new-func
  const factory = new Function(
    'require',
    'module',
    'exports',
    `${source}\n//# sourceURL=${name}`,
  );
  factory(requireFn, mod, mod.exports);
  return mod.exports;
}

async function loadGenerated(): Promise<GeneratedModule> {
  if (!generatedPromise) {
    generatedPromise = (async () => {
      const grpcWeb = normalizeGrpcWeb(grpcWebEsm);
      const jspb = normalizeJspb(jspbEsm);
      patchJspbCompat(jspb);

      const [pbResp, grpcResp] = await Promise.all([
        fetch('./hello_pb.js'),
        fetch('./hello_grpc_web_pb.js'),
      ]);
      if (!pbResp.ok) {
        throw new Error(`failed to load hello_pb.js: ${pbResp.status} ${pbResp.statusText}`);
      }
      if (!grpcResp.ok) {
        throw new Error(`failed to load hello_grpc_web_pb.js: ${grpcResp.status} ${grpcResp.statusText}`);
      }
      const [pbSource, grpcSource] = await Promise.all([pbResp.text(), grpcResp.text()]);

      const pbModule = evalCjsModule(
        pbSource,
        (id) => {
          if (id === 'google-protobuf') return jspb;
          throw new Error(`unsupported require in hello_pb.js: ${id}`);
        },
        'hello_pb.js',
      );

      return evalCjsModule(
        grpcSource,
        (id) => {
          if (id === 'grpc-web') return grpcWeb;
          if (id === './hello_pb.js') return pbModule;
          throw new Error(`unsupported require in hello_grpc_web_pb.js: ${id}`);
        },
        'hello_grpc_web_pb.js',
      ) as GeneratedModule;
    })();
  }
  return generatedPromise;
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

export interface AuthResult {
  token: string;
  user_id: string;
  expires_at: number;
}

export async function register(email: string, password: string): Promise<AuthResult> {
  const grpc = await loadGenerated();
  const client = new grpc.AuthServicePromiseClient(grpcEndpoint(), null, {
    withCredentials: true,
  });
  const req = new grpc.RegisterRequest();
  req.setEmail(email);
  req.setPassword(password);
  const res = await client.register(req, {});
  return { token: res.getToken(), user_id: res.getUserId(), expires_at: Number(res.getExpiresAt()) };
}

export async function login(email: string, password: string): Promise<AuthResult> {
  const grpc = await loadGenerated();
  const client = new grpc.AuthServicePromiseClient(grpcEndpoint(), null, {
    withCredentials: true,
  });
  const req = new grpc.LoginRequest();
  req.setEmail(email);
  req.setPassword(password);
  const res = await client.login(req, {});
  return { token: res.getToken(), user_id: res.getUserId(), expires_at: Number(res.getExpiresAt()) };
}

export async function logout(token = ''): Promise<boolean> {
  const grpc = await loadGenerated();
  const client = new grpc.AuthServicePromiseClient(grpcEndpoint(), null, {
    withCredentials: true,
  });
  const req = new grpc.LogoutRequest();
  req.setToken(token);
  const res = await client.logout(req, {});
  return res.getSuccess();
}
