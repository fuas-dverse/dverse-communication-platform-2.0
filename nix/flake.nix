{
  description = "DVerse CA + Keycloak NixOS flake (step-ca + keycloak IdP for the DVerse platform).";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-25.11";

    sops-nix = {
      url = "github:Mic92/sops-nix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      sops-nix,
      ...
    }@inputs:
    let
      system = "x86_64-linux";
      lib = nixpkgs.lib;
      specialArgs = { inherit system inputs; };
    in
    {
      # ─── Reusable NixOS modules ─────────────────────────────────────────
      # Consumers can import these from their own flake to compose the
      # services into a different host configuration.
      nixosModules = {
        step-ca = import ./modules/services/step-ca.nix;
        keycloak = import ./modules/services/keycloak.nix;
        dverse-ca = import ./hosts/dverse-ca/configuration.nix;
      };

      # ─── Deployable host configurations ────────────────────────────────
      nixosConfigurations = {
        # Production host. Expects:
        #   - a real hosts/dverse-ca/hardware-configuration.nix (the one
        #     shipped here is a stub — replace before deploying)
        #   - secrets/dverse-ca.yaml encrypted to keys listed in .sops.yaml
        dverse-ca = lib.nixosSystem {
          specialArgs = specialArgs;
          modules = [
            sops-nix.nixosModules.sops
            ./hosts/dverse-ca/configuration.nix
            ./hosts/dverse-ca/hardware-configuration.nix
          ];
        };

        # Sops-free VM variant for local testing.
        # Build: nix build .#nixosConfigurations.dverse-ca-vm.config.system.build.vm
        # Run:   ./result/bin/run-dverse-ca-vm
        dverse-ca-vm = lib.nixosSystem {
          specialArgs = specialArgs;
          modules = [
            ./hosts/dverse-ca/vm-test.nix
          ];
        };
      };
    };
}
