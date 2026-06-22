# Sandbox Runtime Management — Spike Artifacts

Reference artifacts for the spikes behind `docs/features/sandbox-runtime-management/`. These files are kept so a future agent can re-run the validation without re-deriving the commands. The findings they produced are written up in `01_ideas.md` (Validation), `02_dev.md` (The flox/Nix Substrate), and `03_e2e.md` (De-Risking Spike Results).

## Files

- `Containerfile` — the Phase A flox-baked base image: `alpine:3.24` + `apk add nix git`, a `/etc/nix/nix.conf` carrying the flox binary cache and the correct `trusted-public-keys`, and `nix profile add github:flox/flox`. Built and validated on 2026-06-23; the resulting image is `sandbox-mio/base:flox` and measures **1.27 GB**.
- `two-machine-repro.sh` — the Phase D two-machine reproduction spike. Installs a package via a flox manifest on `mio-tests` and reproduces it on `mio-tests-remote` from `manifest.lock` alone.

## Headline findings (2026-06-23)

- flox `1.13.0` substitutes cleanly from `cache.flox.dev` during a real `podman build` once the correct key (`flox-cache-public-1:7F4OyH7ZCnFhcze3fJdfyXYLQw/aV7GEed86nQ7IsOs=`) is in `trusted-public-keys`. The wrong key silently falls back to compiling flox from source.
- A package installed on `mio-tests` reproduces on `mio-tests-remote` from a **7,295-byte** manifest transfer; the closure is fetched from the Nix binary cache, not shipped. Both machines end up with the byte-identical store path `/nix/store/zi2bj2hlavv8q743li2s9diqbcpmrf9b-hello-2.12.3` and the same nixpkgs rev `567a49d1913ce81ac6e9582e3553dd90a955875f`.
- The one-time base image transfer between two Podman-machine connections must use a `podman save | podman load` pipe; `podman image scp` between two machine connections is broken on Windows (the connection name is resolved as a DNS host).

## Re-running

The base image must exist on both connections first (see commented step 0 in `two-machine-repro.sh`), then run `two-machine-repro.sh`. The remote machine's rootless Podman socket must be re-activated after any `podman machine` restart (the launcher in `e2e/lib/podman-machine.ts`).
