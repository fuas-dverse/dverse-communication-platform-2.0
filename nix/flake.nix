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
      pkgs = import nixpkgs { inherit system; };
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

      # ─── OCI container images ──────────────────────────────────────────
      # Built with dockerTools.buildLayeredImage for better layer reuse.
      # Output is a gzipped tarball loadable via `docker load` / `podman load`
      # or pushable via `skopeo copy docker-archive:./result docker://...`.
      # Tags are deterministic ("nix"); re-tag on push if you want versioning.
      packages.${system} = {
        # step-ca: entrypoint mirrors the NixOS module's ExecStart
        # (step-ca --password-file <pw> <ca.json>). Operator mounts:
        #   /etc/step-ca/ca.json       (read-only, the ca.json)
        #   /run/secrets/step-ca-password  (read-only, password file)
        #   /var/lib/step-ca           (read-write volume for state/certs)
        # Runs as uid/gid 1000:1000 — chown mounts on the host accordingly.
        oci-step-ca = pkgs.dockerTools.buildLayeredImage {
          name = "dverse/step-ca";
          tag = "nix";
          contents = [
            pkgs.step-ca
            pkgs.step-cli
            pkgs.cacert
            pkgs.bashInteractive
            pkgs.coreutils
          ];
          config = {
            Entrypoint = [ "${pkgs.step-ca}/bin/step-ca" ];
            Cmd = [
              "--password-file"
              "/run/secrets/step-ca-password"
              "/etc/step-ca/ca.json"
            ];
            WorkingDir = "/var/lib/step-ca";
            User = "1000:1000";
            ExposedPorts = {
              "9000/tcp" = { };
            };
            Env = [
              "SSL_CERT_FILE=${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt"
              "STEPPATH=/var/lib/step-ca"
            ];
            Volumes = {
              "/var/lib/step-ca" = { };
            };
          };
        };

        # keycloak: kc.sh entrypoint with `start` (production mode) as the
        # default Cmd. Override to `start-dev` for local testing. Config is
        # supplied via container env vars — see nix/README.md.
        oci-keycloak = pkgs.dockerTools.buildLayeredImage {
          name = "dverse/keycloak";
          tag = "nix";
          contents = [
            pkgs.keycloak
            pkgs.cacert
            pkgs.bashInteractive
            pkgs.coreutils
          ];
          config = {
            Entrypoint = [ "${pkgs.keycloak}/bin/kc.sh" ];
            Cmd = [ "start" ];
            User = "1000:1000";
            ExposedPorts = {
              "8080/tcp" = { };
              "8443/tcp" = { };
            };
            Env = [
              "SSL_CERT_FILE=${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt"
            ];
          };
        };

        # Convenience alias — step-ca is the primary thing this CA host
        # exists for, so `nix build .#` defaults to that image.
        default = self.packages.${system}.oci-step-ca;
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
