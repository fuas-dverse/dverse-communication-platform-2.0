# hosts/dverse-ca/vm-test.nix
#
# Standalone NixOS configuration for local QEMU VM testing.
# Does NOT use sops — password is plain text (test only).
# Build: nix build .#nixosConfigurations.dverse-ca-vm.config.system.build.vm
# Run:   ./result/bin/run-dverse-ca-vm
# Verify: step ca health --ca-url https://localhost:9000 \
#           --root /var/lib/step-ca/certs/root_ca.crt

{
  pkgs,
  lib,
  modulesPath,
  config,
  ...
}:
{
  imports = [
    (modulesPath + "/profiles/qemu-guest.nix")
    ../../modules/services/step-ca.nix
  ];

  nixpkgs.hostPlatform = "x86_64-linux";
  networking.hostName = "dverse-ca";
  time.timeZone = "Europe/Amsterdam";

  # Passwordless root login for console / SSH access in the test VM
  users.users.root.initialPassword = "root";
  services.openssh = {
    enable = true;
    settings = {
      PermitRootLogin = "yes";
      PasswordAuthentication = true;
    };
  };

  networking.firewall = {
    enable = true;
    allowedTCPPorts = [ 22 ];
  };

  environment.systemPackages = with pkgs; [
    curl
    jq
  ];

  # Plain-text password file — never use this pattern outside a test VM
  environment.etc."step-ca-password".text = "dverse-ca-test-password\n";

  # ── Step-CA module options ───────────────────────────────────────────────
  services.step-ca = {
    enable = true;
    port = 9000;
    dnsNames = [
      "dverse-ca"
      "localhost"
    ];
    passwordFile = "/etc/step-ca-password";
  };

  # ── CA bootstrap ────────────────────────────────────────────────────────
  # One-shot service that runs `step ca init` on first boot and places the
  # generated key material in /var/lib/step-ca/.  Subsequent reboots skip
  # it because the sentinel file already exists.

  systemd.services.step-ca-init = {
    description = "Bootstrap Step-CA key material (first boot)";
    wantedBy = [ "multi-user.target" ];
    before = [ "step-ca.service" ];
    requiredBy = [ "step-ca.service" ];
    path = with pkgs; [
      step-cli
      coreutils
      gnused
    ];

    environment.STEPPATH = "/tmp/step-ca-init";

    serviceConfig = {
      Type = "oneshot";
      RemainAfterExit = true;
    };

    script =
      let
        port = toString config.services.step-ca.port;
        dnsFlags = lib.concatMapStringsSep " " (n: "--dns ${n}") config.services.step-ca.dnsNames;
        pwdFile = config.services.step-ca.passwordFile;
      in
      ''
        if [ -f /var/lib/step-ca/config/ca.json ]; then
          echo "CA already initialised, skipping."
          exit 0
        fi

        rm -rf "$STEPPATH"
        mkdir -p "$STEPPATH"

        step ca init \
          --deployment-type standalone \
          --name "DVerse CA" \
          ${dnsFlags} \
          --address ":${port}" \
          --provisioner "dverse-admin" \
          --password-file ${pwdFile}

        mkdir -p /var/lib/step-ca/{certs,secrets,db,config}

        cp "$STEPPATH/certs/root_ca.crt"           /var/lib/step-ca/certs/
        cp "$STEPPATH/certs/intermediate_ca.crt"   /var/lib/step-ca/certs/
        cp "$STEPPATH/secrets/intermediate_ca_key" /var/lib/step-ca/secrets/

        sed "s|$STEPPATH|/var/lib/step-ca|g" \
          "$STEPPATH/config/ca.json" > /var/lib/step-ca/config/ca.json

        chown -R step-ca:step-ca /var/lib/step-ca/
        rm -rf "$STEPPATH"
      '';
  };

  # The module defines step-ca.service with after = [ "network.target" ].
  # Wire it so it also waits for the init service.
  systemd.services.step-ca = {
    after = [
      "network.target"
      "step-ca-init.service"
    ];
    requires = [ "step-ca-init.service" ];
  };

  system.stateVersion = "25.11";
}
