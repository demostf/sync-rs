{
  lib,
  config,
  ...
}: let
  cfg = config.services.demostf.sync;
in {
  options = {
    services.demostf.sync = with lib; {
      enable = mkEnableOption "demostf sync";
      package = mkOption {
        type = types.package;
        defaultText = literalExpression "pkgs.demostf-sync";
        description = "package to use";
      };
      socket = mkOption {
        type = types.str;
        default = "/var/run/demostf-sync/sync.socket";
      };
    };
  };

  config = lib.mkIf cfg.enable {
    systemd.services.demostf-sync = {
      wantedBy = ["multi-user.target"];
      environment = {
        SOCKET = cfg.socket;
      };

      serviceConfig = {
        DynamicUser = true;
        ExecStart = "${cfg.package}/bin/sync";
        Restart = "on-failure";

        PrivateTmp = true;
        PrivateUsers = true;
        ProtectSystem = "strict";
        ProtectHome = true;
        NoNewPrivileges = true;
        PrivateDevices = true;
        ProtectClock = true;
        CapabilityBoundingSet = true;
        ProtectKernelLogs = true;
        ProtectControlGroups = true;
        SystemCallArchitectures = "native";
        ProtectKernelModules = true;
        RestrictNamespaces = true;
        MemoryDenyWriteExecute = true;
        ProtectHostname = true;
        LockPersonality = true;
        ProtectKernelTunables = true;
        DevicePolicy = "closed";
        RestrictAddressFamilies = ["AF_UNIX"];
        RestrictRealtime = true;
        ProcSubset = "pid";
        ProtectProc = "invisible";
        SystemCallFilter = ["@system-service" "~@resources" "~@privileged"];
        UMask = "0007";
        IPAddressDeny = "any";
        RuntimeDirectory = "demostf-sync";
      };
    };
  };
}
