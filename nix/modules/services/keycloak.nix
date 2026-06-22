{
  config,
  lib,
  ...
}:

let
  cfg = config.services.keycloak-server;
in
{
  options.services.keycloak-server = {
    enable = lib.mkEnableOption "Keycloak identity provider";

    hostname = lib.mkOption {
      type = lib.types.str;
      description = "Public hostname Keycloak is reachable at (e.g. auth.example.com).";
    };

    adminPasswordEnvFile = lib.mkOption {
      type = lib.types.path;
      description = ''
        Path to a file (readable by root) containing:
          KC_BOOTSTRAP_ADMIN_USERNAME=admin
          KC_BOOTSTRAP_ADMIN_PASSWORD=<secret>
        Systemd reads this before dropping privileges, so a root-owned 0400 file works.
      '';
    };

    databasePasswordFile = lib.mkOption {
      type = lib.types.path;
      description = "Path to a file containing the PostgreSQL password for Keycloak.";
    };

    acmeEmail = lib.mkOption {
      type = lib.types.str;
      description = "Email address used for Let's Encrypt ACME registration.";
    };
  };

  config = lib.mkIf cfg.enable {
    services.keycloak = {
      enable = true;
      settings = {
        hostname = cfg.hostname;
        http-enabled = true;
        http-port = 8080;
        proxy-headers = "xforwarded";
      };
      database = {
        type = "postgresql";
        createLocally = true;
        passwordFile = cfg.databasePasswordFile;
      };
    };

    systemd.services.keycloak.serviceConfig.EnvironmentFile = cfg.adminPasswordEnvFile;

    services.nginx = {
      enable = true;
      virtualHosts.${cfg.hostname} = {
        enableACME = true;
        forceSSL = true;
        locations."/" = {
          proxyPass = "http://127.0.0.1:8080";
          proxyWebsockets = true;
          extraConfig = ''
            proxy_set_header Host $host;
            proxy_set_header X-Real-IP $remote_addr;
            proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
            proxy_set_header X-Forwarded-Proto $scheme;
          '';
        };
      };
    };

    security.acme = {
      acceptTerms = true;
      defaults.email = cfg.acmeEmail;
    };

    networking.firewall.allowedTCPPorts = [
      80
      443
    ];
  };
}
