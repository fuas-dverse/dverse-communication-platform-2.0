# hosts/dverse-ca/configuration.nix
#
# Top-level NixOS configuration for the production `dverse-ca` host. Composes
# the step-ca and keycloak service modules with sops-nix-managed secrets and
# a minimal system baseline (openssh + firewall + sops tooling).
#
# Anything personal (user accounts, ssh keys, tailscale, editor configs) that
# lived in the source repo has been stripped — operators are expected to add
# their own users / authorized keys via the `dverseCa.operator*` options or by
# layering an additional module.

{
  config,
  lib,
  pkgs,
  ...
}:
{
  imports = [
    ../../modules/services/step-ca.nix
    ../../modules/services/keycloak.nix
    ./step-ca.nix
    ./keycloak.nix
  ];

  options.dverseCa = {
    operatorSshKeys = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [ ];
      example = [ "ssh-ed25519 AAAA... operator@example.com" ];
      description = ''
        SSH public keys authorised to log in as root on the host. Replace the
        default empty list with your own keys before deploying; otherwise the
        only way in is via console.
      '';
    };
  };

  config = {
    networking.hostName = lib.mkDefault "dverse-ca";
    time.timeZone = lib.mkDefault "Europe/Amsterdam";

    users.users.root.openssh.authorizedKeys.keys = config.dverseCa.operatorSshKeys;

    # ─── sops-nix baseline ────────────────────────────────────────────────
    # Secrets are decrypted at activation time using the host's ssh ed25519
    # key, converted to an age identity by sops-nix. No separate keyfile to
    # provision.
    environment.systemPackages = with pkgs; [
      sops
      age
      ssh-to-age
      neovim
      git
      curl
    ];

    sops = {
      defaultSopsFile = ../../secrets/dverse-ca.yaml;
      defaultSopsFormat = "yaml";
      age.sshKeyPaths = [ "/etc/ssh/ssh_host_ed25519_key" ];
      age.generateKey = false;
    };

    # ─── Minimal system baseline ──────────────────────────────────────────
    services.openssh = {
      enable = true;
      ports = [ 22 ];
      settings = {
        PasswordAuthentication = false;
        KbdInteractiveAuthentication = false;
        PermitRootLogin = "prohibit-password";
      };
    };

    networking.firewall = {
      enable = true;
      allowedTCPPorts = [ 22 ];
    };

    services.logrotate.checkConfig = false;
    boot.tmp.cleanOnBoot = true;
    zramSwap.enable = true;

    system.stateVersion = lib.mkDefault "25.11";
  };
}
