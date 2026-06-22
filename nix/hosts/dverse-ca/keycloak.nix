{ config, pkgs, ... }:
{
  sops.secrets.keycloak-admin-password = {
    sopsFile = ../../secrets/dverse-ca.yaml;
  };

  sops.secrets.keycloak-db-password = {
    sopsFile = ../../secrets/dverse-ca.yaml;
  };

  sops.secrets.keycloak-step-ca-secret = {
    sopsFile = ../../secrets/dverse-ca.yaml;
  };

  sops.secrets.keycloak-registration-secret = {
    sopsFile = ../../secrets/dverse-ca.yaml;
  };

  sops.templates.keycloak-admin-env = {
    content = ''
      KC_BOOTSTRAP_ADMIN_USERNAME=admin
      KC_BOOTSTRAP_ADMIN_PASSWORD=${config.sops.placeholder.keycloak-admin-password}
    '';
    mode = "0400";
  };

  # Full realm definition rendered at boot with secrets injected from sops.
  # An ExecStartPre hook symlinks this into /run/keycloak/data/import/ before
  # kc.sh --import-realm runs. Keycloak skips import if the realm already exists.
  sops.templates.keycloak-dverse-realm = {
    content = builtins.toJSON {
      realm = "dverse";
      enabled = true;

      clients = [
        {
          clientId = "step-ca";
          name = "Step CA";
          enabled = true;
          protocol = "openid-connect";
          publicClient = false;
          secret = config.sops.placeholder.keycloak-step-ca-secret;
          directAccessGrantsEnabled = true;
          standardFlowEnabled = false;
          implicitFlowEnabled = false;
          serviceAccountsEnabled = false;
        }
        {
          clientId = "dverse-registration";
          name = "DVerse Registration";
          enabled = true;
          protocol = "openid-connect";
          publicClient = false;
          secret = config.sops.placeholder.keycloak-registration-secret;
          serviceAccountsEnabled = true;
          standardFlowEnabled = false;
          directAccessGrantsEnabled = false;
          implicitFlowEnabled = false;
        }
      ];

      users = [
        {
          username = "service-account-dverse-registration";
          enabled = true;
          serviceAccountClientId = "dverse-registration";
          clientRoles."realm-management" = [ "manage-users" ];
        }
      ];

      userProfileConfig = {
        attributes = [
          {
            name = "username";
            displayName = "\${username}";
            validations = { length = { min = 3; max = 255; }; username-prohibited-characters = {}; up-username-not-idn-homograph = {}; };
            permissions = { view = [ "admin" "user" ]; edit = [ "admin" "user" ]; };
            multivalued = false;
          }
          {
            name = "email";
            displayName = "\${email}";
            validations = { email = {}; length = { max = 255; }; };
            required = { roles = [ "user" ]; };
            permissions = { view = [ "admin" "user" ]; edit = [ "admin" "user" ]; };
            multivalued = false;
          }
          {
            name = "firstName";
            displayName = "\${firstName}";
            validations = { length = { max = 255; }; person-name-prohibited-characters = {}; };
            permissions = { view = [ "admin" "user" ]; edit = [ "admin" "user" ]; };
            multivalued = false;
          }
          {
            name = "lastName";
            displayName = "\${lastName}";
            validations = { length = { max = 255; }; person-name-prohibited-characters = {}; };
            permissions = { view = [ "admin" "user" ]; edit = [ "admin" "user" ]; };
            multivalued = false;
          }
        ];
        groups = [
          { name = "user-metadata"; displayHeader = "User metadata"; displayDescription = "Attributes, which refer to user metadata"; }
        ];
      };
    };
    mode = "0444";
  };

  services.keycloak-server = {
    enable = true;
    hostname = "auth.dverse.yordanmitev.me";
    adminPasswordEnvFile = config.sops.templates.keycloak-admin-env.path;
    databasePasswordFile = config.sops.secrets.keycloak-db-password.path;
    acmeEmail = "yordanmitev@yordanmitev.me";
  };

  # realmFiles tells the nixpkgs module to add --import-realm to kc.sh start.
  # The tmpfiles symlink it tries to create fails due to RuntimeDirectory ownership, but
  # ExecStartPre below creates the symlink correctly before kc.sh runs.
  services.keycloak.realmFiles = [
    config.sops.templates.keycloak-dverse-realm.path
  ];

  # ExecStartPre runs as the keycloak user after RuntimeDirectory creates /run/keycloak/.
  # It places the sops-rendered realm JSON where kc.sh --import-realm expects it.
  # RuntimeDirectory wipes the dir on every service stop, so we recreate on each start.
  systemd.services.keycloak.serviceConfig.ExecStartPre =
    let
      setupImport = pkgs.writeShellScript "keycloak-setup-import" ''
        mkdir -p /run/keycloak/data/import
        ln -sf ${config.sops.templates.keycloak-dverse-realm.path} \
          /run/keycloak/data/import/dverse-realm.json
      '';
    in
    [ "${setupImport}" ];
}
