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

## Building OCI container images

The flake also exposes two container images for operators who want to
deploy step-ca or Keycloak outside of a full NixOS host (e.g. into an
existing Kubernetes / Nomad / docker-compose setup). The images are
reproducible Nix builds — they are tagged `nix` and **not** stamped with
a git SHA. Re-tag on push if you want versioning.

| Output                     | What it builds            | Approx size (gzipped) |
| -------------------------- | ------------------------- | --------------------- |
| `.#oci-step-ca` (default)  | `dverse/step-ca:nix`      | ~54 MiB               |
| `.#oci-keycloak`           | `dverse/keycloak:nix`     | ~609 MiB              |

Build:

    cd nix
    nix build .#oci-step-ca       # ./result is a gzipped docker-archive tar
    nix build .#oci-keycloak

Load into a local engine:

    docker load < result
    # or
    podman load < result

Push to a registry (no engine required) with skopeo:

    skopeo copy docker-archive:./result \
      docker://registry.example.com/dverse/step-ca:nix

### step-ca image

Mirrors the NixOS module's invocation
(`step-ca --password-file <pw> <ca.json>`). Both images run as uid/gid
`1000:1000` — chown your host mounts accordingly.

Expected mounts:

| Container path                  | Purpose                                    |
| ------------------------------- | ------------------------------------------ |
| `/etc/step-ca/ca.json`          | The `ca.json` produced by `step ca init`   |
| `/run/secrets/step-ca-password` | File containing the intermediate-key password |
| `/var/lib/step-ca`              | State volume (certs, db, secrets/)         |

Exposed port: `9000/tcp` (HTTPS).

Minimal run example:

    docker run -d --name step-ca \
      -p 9000:9000 \
      -v /srv/step-ca/ca.json:/etc/step-ca/ca.json:ro \
      -v /srv/step-ca/password:/run/secrets/step-ca-password:ro \
      -v step-ca-data:/var/lib/step-ca \
      dverse/step-ca:nix

To override the entrypoint for ops (e.g. to run `step ca health`), pass
`--entrypoint /bin/step`.

### keycloak image

Entrypoint is `kc.sh` with default `Cmd = ["start"]` (production mode).
Override to `start-dev` for local testing. **Config is supplied via
container env vars** — nothing is baked in.

Minimum env vars for a production-mode start:

| Variable                   | Example                                   |
| -------------------------- | ----------------------------------------- |
| `KC_DB`                    | `postgres`                                |
| `KC_DB_URL`                | `jdbc:postgresql://db:5432/keycloak`      |
| `KC_DB_USERNAME`           | `keycloak`                                |
| `KC_DB_PASSWORD`           | `<secret>`                                |
| `KC_HOSTNAME`              | `auth.example.com`                        |
| `KEYCLOAK_ADMIN`           | `admin` (only honoured on first boot)     |
| `KEYCLOAK_ADMIN_PASSWORD`  | `<secret>` (only honoured on first boot)  |

Exposed ports: `8080/tcp`, `8443/tcp`.

Postgres is **not** bundled — bring your own.

Minimal local-testing run:

    docker run -d --name keycloak \
      -p 8080:8080 \
      -e KEYCLOAK_ADMIN=admin \
      -e KEYCLOAK_ADMIN_PASSWORD=admin \
      dverse/keycloak:nix start-dev

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
