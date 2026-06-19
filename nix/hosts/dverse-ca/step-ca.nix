# hosts/dverse-ca/step-ca.nix
#
# Host-specific Step-CA configuration. Enables the shared module and wires
# up the sops-managed password secret. Imported from configuration.nix.

{ config, ... }:
{
  sops.secrets.step-ca-password = {
    sopsFile = ../../secrets/dverse-ca.yaml;
    owner = "step-ca";
    group = "step-ca";
    mode = "0400";
  };

  services.step-ca = {
    enable = true;
    port = 9000;
    dnsNames = [
      "dverse-ca"
      "dverse-ca.local"
      "localhost"
      "ca.dverse.yordanmitev.me"
    ];
    passwordFile = config.sops.secrets.step-ca-password.path;
  };
}
