---
type: spike
title: "Spike Artefacts — Proxy Remote Deploy Phase A"
feature: proxy-remote-deploy
---
# Spike Artefacts — Proxy Remote Deploy Phase A

Run on 2026-11-24 against the `mio-tests-remote` connection (second rootless WSL Podman machine; Podman 5.8.6 server, netavark, `docker.io/ironsh/iron-proxy:0.56.0`, `localhost/sandbox-mio/base:10`). Reproduce with [remote_spike.sh](remote_spike.sh). All `spike-prd-*` resources were removed afterwards. Check 4 (a real Linux host) was not run: no such host was available, so it stays recorded in [../04_improvements.md](../04_improvements.md).

## Results

| # | Check | Result | Evidence |
|---|---|---|---|
| 1 | Internal network, dual-homed sidecar and in-container config over `--connection` | PASS with change | `podman cp <dir>/. <sidecar>:/etc/iron-proxy/config` fails ("could not be found") on a created container because `/etc/iron-proxy` does not exist. Copying one staged tree (holding `config/` and `certs/`) onto `<sidecar>:/etc/iron-proxy` creates the path and works. Files land mode 0666/0777 and root-owned, readable by the root user iron-proxy runs as. The sidecar boots and serves; a copied-in `config.next.yaml` renamed over `config.yaml` reloads (`management reload succeeded`) |
| 2 | tmpfs at the secrets path | PASS | `--tmpfs /etc/iron-proxy/secrets:rw,mode=0755,size=1m` mounts over the copied tree; a file written with `exec -i … sh -c 'cat > .K.tmp && mv'` is readable; `podman cp` also writes into the tmpfs of a running container. Decision 1 stays (b) |
| 3 | Sandbox on the internal network over `--connection` | PASS | `example.com` allowed (200), a non-allowlisted host rejected (403), a raw IP over HTTPS fails (curl exit 7), `exec -i … mio-sshd` resolves and `exec -i` carries stdin, `podman logs --timestamps --since` returns the audit rows over the connection |
| 4 | A real Linux host | NOT RUN | No host available; the Decision 6 floor (Podman 4.0+, netavark) rests on the multiple-`--network` flag support documented upstream, not on a measured old Podman |

The CA used was generated with `openssl` carrying `keyUsage=keyCertSign`; iron-proxy refuses a CA without it ("CA certificate missing KeyUsageCertSign"), which the app's `iron::generate_ca` already satisfies.
