{
  config,
  lib,
  pkgs,
  ...
}:

let
  cfg = config.services.step-ca;
in
{
  disabledModules = [ "services/security/step-ca.nix" ];

  options.services.step-ca = {
    enable = lib.mkEnableOption "Step-CA certificate authority";

    port = lib.mkOption {
      type = lib.types.port;
      default = 9000;
      description = "Port the CA HTTPS listener binds to.";
    };

    dnsNames = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [ "localhost" ];
      description = "Hostnames the CA TLS certificate covers.";
    };

    passwordFile = lib.mkOption {
      type = lib.types.path;
      description = "Path to a file containing the intermediate CA key password.";
    };

    configFile = lib.mkOption {
      type = lib.types.path;
      default = "/var/lib/step-ca/config/ca.json";
      description = "Path to the ca.json produced by step ca init.";
    };
  };

  config = lib.mkIf cfg.enable {
    environment.systemPackages = [
      pkgs.step-ca
      pkgs.step-cli
      pkgs.openssl
    ];

    users.users.step-ca = {
      isSystemUser = true;
      group = "step-ca";
      home = "/var/lib/step-ca";
      createHome = false;
    };
    users.groups.step-ca = { };

    networking.firewall.allowedTCPPorts = [ cfg.port ];

    systemd.services.step-ca = {
      description = "Step-CA certificate authority";
      wantedBy = [ "multi-user.target" ];
      after = [ "network.target" ];
      serviceConfig = {
        Type = "simple";
        User = "step-ca";
        Group = "step-ca";
        StateDirectory = "step-ca";
        UMask = "0077";
        WorkingDirectory = "/var/lib/step-ca";
        ExecStart = "${pkgs.step-ca}/bin/step-ca --password-file ${cfg.passwordFile} ${cfg.configFile}";
        Restart = "on-failure";
        RestartSec = "5s";
        NoNewPrivileges = true;
        ProtectSystem = "full";
        ProtectHome = true;
        PrivateTmp = true;
        RestrictAddressFamilies = "AF_INET AF_INET6";
      };
    };
  };
}
