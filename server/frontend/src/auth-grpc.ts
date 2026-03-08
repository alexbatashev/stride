/**
 * gRPC-Web client for the AuthService.
 *
 * The Bazel-generated protobuf stubs (hello_pb.js, hello_grpc_web_pb.js) are
 * CommonJS modules. They cannot be statically imported from an ES module
 * without a bundler. Instead we fetch them as text and evaluate them in a
 * minimal CJS environment — matching the original approach.
 */

// ---------------------------------------------------------------------------
// Types for the CJS modules loaded at runtime
// ---------------------------------------------------------------------------

interface AuthServiceClient {
  register(req: RegisterRequestMsg, meta: Record<string, string>): Promise<AuthReplyMsg>;
  login(req: LoginRequestMsg, meta: Record<string, string>): Promise<AuthReplyMsg>;
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

interface GeneratedModule {
  AuthServicePromiseClient: new (
    endpoint: string,
    credentials: null,
    options: null,
  ) => AuthServiceClient;
  RegisterRequest: new () => RegisterRequestMsg;
  LoginRequest: new () => LoginRequestMsg;
}

// eslint-disable-next-line @typescript-eslint/no-explicit-any
type AnyNs = any;

// ---------------------------------------------------------------------------
// CJS loader
// ---------------------------------------------------------------------------

let generatedPromise: Promise<GeneratedModule> | null = null;

function grpcEndpoint(): string {
  const w = window as Window & { FRIDAY_GRPC_ENDPOINT?: string };
  if (w.FRIDAY_GRPC_ENDPOINT) return w.FRIDAY_GRPC_ENDPOINT;
  const url = new URL(window.location.href);
  url.port = '50051';
  url.pathname = '';
  url.search = '';
  url.hash = '';
  return url.origin;
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

function patchJspbCompat(jspb: AnyNs): void {
  const proto = jspb?.BinaryReader?.prototype;
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
      const grpcWebUrl = 'https://esm.sh/grpc-web@1.5.0';
      const jspbUrl = 'https://esm.sh/google-protobuf@3.21.4';
      // eslint-disable-next-line @typescript-eslint/no-implied-eval
      const [grpcWebEsm, jspbEsm] = await Promise.all([
        import(/* @vite-ignore */ grpcWebUrl),
        import(/* @vite-ignore */ jspbUrl),
      ]);

      const grpcWeb = normalizeGrpcWeb(grpcWebEsm);
      const jspb = normalizeJspb(jspbEsm);
      patchJspbCompat(jspb);

      const [pbSource, grpcSource] = await Promise.all([
        fetch('./hello_pb.js').then((r) => r.text()),
        fetch('./hello_grpc_web_pb.js').then((r) => r.text()),
      ]);

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
  const client = new grpc.AuthServicePromiseClient(grpcEndpoint(), null, null);
  const req = new grpc.RegisterRequest();
  req.setEmail(email);
  req.setPassword(password);
  const res = await client.register(req, {});
  return { token: res.getToken(), user_id: res.getUserId(), expires_at: Number(res.getExpiresAt()) };
}

export async function login(email: string, password: string): Promise<AuthResult> {
  const grpc = await loadGenerated();
  const client = new grpc.AuthServicePromiseClient(grpcEndpoint(), null, null);
  const req = new grpc.LoginRequest();
  req.setEmail(email);
  req.setPassword(password);
  const res = await client.login(req, {});
  return { token: res.getToken(), user_id: res.getUserId(), expires_at: Number(res.getExpiresAt()) };
}
