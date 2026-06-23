# NixOS module exposing the Stride server as a hardened systemd service.
# Non-secret configuration is written as TOML from `services.stride.settings`;
# secrets (STRIDE_JWT_SECRET and friends) are injected via `environmentFile`
# so they never land in the world-readable Nix store.
{
  config,
  lib,
  pkgs,
  ...
}:
let
  cfg = config.services.stride;
  format = pkgs.formats.toml { };
  configFile =
    if cfg.configFile != null then cfg.configFile else format.generate "stride-config.toml" cfg.settings;
  listenPort = lib.toInt (lib.last (lib.splitString ":" cfg.settings.server.listen_addr));
in
{
  options.services.stride = {
    enable = lib.mkEnableOption "the Stride agent server";

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.stride-server;
      defaultText = lib.literalExpression "pkgs.stride-server";
      description = "The stride-server package to run.";
    };

    user = lib.mkOption {
      type = lib.types.str;
      default = "stride";
      description = "User the service runs as. A system user is created when left at the default.";
    };

    group = lib.mkOption {
      type = lib.types.str;
      default = "stride";
      description = "Group the service runs as.";
    };

    stateDir = lib.mkOption {
      type = lib.types.path;
      default = "/var/lib/stride";
      description = "Writable state directory (SQLite database, local files).";
    };

    environmentFile = lib.mkOption {
      type = lib.types.listOf lib.types.path;
      default = [ ];
      example = [ "/run/secrets/stride.env" ];
      description = ''
        EnvironmentFiles read by the unit. Put secrets here, one `KEY=value` per
        line. At minimum set `STRIDE_JWT_SECRET` (>= 32 bytes). Other useful
        keys: `STRIDE_EMAIL_ENCRYPTION_KEY`, `STRIDE_<PROVIDER>_API_KEY`,
        `STRIDE_MCP_<NAME>_TOKEN`, `STRIDE_BRAVE_API_KEY`,
        `STRIDE_TELEGRAM_BOT_API_KEY`, `STRIDE_FIRECRAWL_API_KEY`.
      '';
    };

    openFirewall = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = "Open the TCP port from `settings.server.listen_addr` in the firewall.";
    };

    configFile = lib.mkOption {
      type = lib.types.nullOr lib.types.path;
      default = null;
      description = "Escape hatch: use this config.toml verbatim instead of rendering `settings`.";
    };

    settings = lib.mkOption {
      type = format.type;
      default = { };
      example = lib.literalExpression ''
        {
          providers.openai = { kind = "OpenAI"; url = "https://api.openai.com/v1"; };
          models.gpt = { slug = "gpt-4.1"; provider = "openai"; reasoning_effort = "high"; };
          server.allow_registration = false;
        }
      '';
      description = ''
        Contents of config.toml. Mirrors the server's TOML schema
        (`providers`, `models`, `server`, `tools`, `mcp`). Tokens are best left
        out and supplied through `environmentFile` instead.
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    assertions = [
      {
        assertion = cfg.environmentFile != [ ] || cfg.configFile != null;
        message = ''
          services.stride: set `environmentFile` to a file defining STRIDE_JWT_SECRET
          (the server refuses to start without a strong secret).
        '';
      }
    ];

    # providers/models are required TOML tables; default them so config.toml
    # always parses even before the operator fills them in.
    services.stride.settings = {
      providers = lib.mkDefault { };
      models = lib.mkDefault { };
      server.db_path = lib.mkDefault "${cfg.stateDir}/server.db";
      server.listen_addr = lib.mkDefault "0.0.0.0:3000";
    };

    users.users = lib.mkIf (cfg.user == "stride") {
      stride = {
        isSystemUser = true;
        group = cfg.group;
        home = cfg.stateDir;
        description = "Stride agent server";
      };
    };
    users.groups = lib.mkIf (cfg.group == "stride") { stride = { }; };

    systemd.tmpfiles.rules = [ "d ${cfg.stateDir} 0750 ${cfg.user} ${cfg.group} - -" ];

    networking.firewall.allowedTCPPorts = lib.mkIf cfg.openFirewall [ listenPort ];

    systemd.services.stride = {
      description = "Stride agent server";
      wantedBy = [ "multi-user.target" ];
      after = [ "network-online.target" ];
      wants = [ "network-online.target" ];

      serviceConfig = {
        ExecStart = "${lib.getExe cfg.package} -c ${configFile}";
        User = cfg.user;
        Group = cfg.group;
        EnvironmentFile = cfg.environmentFile;
        WorkingDirectory = cfg.stateDir;
        Restart = "on-failure";
        RestartSec = 5;

        # Hardening. The Python sandbox (eryx) JITs WASM, so writable+executable
        # memory must stay allowed; everything else is locked down.
        MemoryDenyWriteExecute = false;
        NoNewPrivileges = true;
        ProtectSystem = "strict";
        ProtectHome = true;
        PrivateTmp = true;
        ReadWritePaths = [ cfg.stateDir ];
        ProtectKernelTunables = true;
        ProtectKernelModules = true;
        ProtectControlGroups = true;
        ProtectClock = true;
        RestrictSUIDSGID = true;
        LockPersonality = true;
        RestrictRealtime = true;
        SystemCallFilter = [
          "@system-service"
          "~@privileged"
        ];
        SystemCallErrorNumber = "EPERM";
      };
    };
  };
}
