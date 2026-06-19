# nix/ — DVerse CA + Keycloak NixOS flake

Self-contained Nix flake that builds the **dverse-ca** host: a step-ca
certificate authority and a Keycloak identity provider pre-loaded with the
`dverse` realm, both fronted by nginx + ACME for the production deployment.

This directory was extracted from a personal multi-host nix-config so that
the dverse project can build and deploy its own CA/IdP without depending on
the upstream personal repo.

## Layout

    nix/
    ├── flake.nix                          # inputs + nixosModules + nixosConfigurations
    ├── .sops.yaml                         # sops recipient template (edit before use)
    ├── README.md
    ├── modules/services/
    │   ├── step-ca.nix                    # custom services.step-ca module
    │   └── keycloak.nix                   # custom services.keycloak-server wrapper
    ├── hosts/dverse-ca/
    │   ├── configuration.nix              # top-level host config (composes everything)
    │   ├── step-ca.nix                    # step-ca host wiring + sops secret
    │   ├── keycloak.nix                   # keycloak host wiring + realm import
    │   ├── vm-test.nix                    # sops-free QEMU test variant
    │   └── hardware-configuration.nix     # **stub** — replace per-target
    └── secrets/
        └── dverse-ca.example.yaml         # plaintext schema template

## Inputs

| Input    | Channel / ref          |
| -------- | ---------------------- |
| nixpkgs  | `nixos-25.11`          |
| sops-nix | `Mic92/sops-nix` (HEAD)|

Run `nix flake update` (or `nix flake lock --update-input <name>`) under
`nix/` to refresh.

## Building the test VM

The VM variant has no sops dependency and uses a baked-in plaintext password
for step-ca. It is for local smoke-testing only.

    cd nix
    nix build .#nixosConfigurations.dverse-ca-vm.config.system.build.vm
    ./result/bin/run-dverse-ca-vm

Inside the VM (root/root login):

    step ca health --ca-url https://localhost:9000 \
      --root /var/lib/step-ca/certs/root_ca.crt

## Deploying the real host

1. **Replace the hardware module.** The shipped
   `hosts/dverse-ca/hardware-configuration.nix` is a non-functional stub.
   Generate the real one on the target machine:

       nixos-generate-config --show-hardware-config \
         > nix/hosts/dverse-ca/hardware-configuration.nix

2. **Add operator ssh keys.** Either set the option directly on the host
   config or layer a module:

       dverseCa.operatorSshKeys = [
         "ssh-ed25519 AAAA... you@example.com"
       ];

   Without this, the only way in after deploy is the serial console.

3. **Bootstrap secrets** (see next section).

4. **Build / deploy** as usual:

       nixos-rebuild switch --flake .#dverse-ca --target-host root@<host>

## Secrets bootstrap

`sops-nix` decrypts secrets at activation time using the target host's
`/etc/ssh/ssh_host_ed25519_key`, transparently converted to an age identity.
No separate keyfile is provisioned on the server.

Steps:

1. Generate an admin age key on your workstation if you don't have one:

       age-keygen -o ~/.config/sops/age/keys.txt

2. Derive the server's age public key from its ssh host key (run on the
   target after a base install):

       nix run nixpkgs#ssh-to-age -- -i /etc/ssh/ssh_host_ed25519_key.pub

3. Edit `nix/.sops.yaml`: replace `age1REPLACEME...` lines with the two
   real recipients (workstation pubkey + server pubkey).

4. Copy the schema template and fill in real values:

       cp nix/secrets/dverse-ca.example.yaml nix/secrets/dverse-ca.yaml
       $EDITOR nix/secrets/dverse-ca.yaml
       sops --encrypt --in-place nix/secrets/dverse-ca.yaml

5. Commit `nix/secrets/dverse-ca.yaml` (encrypted). Do **not** commit the
   plaintext copy.

If you later rotate recipients, update `.sops.yaml` and run:

    sops updatekeys nix/secrets/dverse-ca.yaml

## What's intentionally not here

This flake was scrubbed of bits that only made sense in the source personal
repo:

- Personal user accounts and ssh keys (operator keys are now an explicit
  `dverseCa.operatorSshKeys` option, defaulting to empty).
- Personal editor / shell config (tmux, bash, neovim modules).
- Other hosts and unused inputs (`home-manager`, `nixvim`,
  `nixpkgs-unstable`).
- The real `dverse-ca.yaml` ciphertext — that file was encrypted to the
  original author's keys and would be unusable here. Re-create it locally
  with your own recipients per the bootstrap steps above.

## Provenance

Original source: `/stuff/programming/nix-config/` (personal repo, not
published). Modules are byte-for-byte copies; host wiring has been adapted
to drop personal-repo-specific imports and to make the operator-supplied
bits explicit options instead of inline values.
