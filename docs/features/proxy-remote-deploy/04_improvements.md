---
type: improvements
title: "Proxy Remote Deploy: Deferred Improvements"
feature: proxy-remote-deploy
---
# Proxy Remote Deploy: Deferred Improvements

Ideas noticed while planning `01_ideas.md`/`02_dev.md` that are worthwhile but out of scope for the
initial "keep the proxy across a deploy" feature.

## Cross-connection sweep for orphaned remote proxy resources

`02_dev.md` Resolved Decision 8 keeps `proxy::sweep` (`src-tauri/src/proxy/mod.rs`) local-only: a
remote proxy sidecar's lifecycle is handled entirely by `deploy`/`undeploy`/`destroy`'s explicit calls,
not a periodic scan. A process crash or forced-quit mid-deploy (after `create_proxy_remote` succeeds
but before the sandbox container or CA injection completes) could leave a remote `<sandbox>-proxy`
container, its network, and its state directory (CA private key included) on the target connection
with nothing on the local side to know it exists. A future iteration could extend the startup sweep
to also enumerate registered remote connections and remove orphaned proxy resources whose base
sandbox is absent from that connection's own `podman ps`. Deferred because it requires enumerating
every registered connection on every app startup — an unbounded, network-dependent operation the
current local-only sweep deliberately avoids — and because the crash window it protects against is
narrow and already leaves recoverable state (a stopped container, not silently corrupted data).

## Unified remote-provisioning progress reporting

`02_dev.md` Resolved Decision 5 notes that `ensure_proxy_image`'s existing `activity::TaskRegistry`
reporting (`src-tauri/src/proxy/mod.rs`) works unchanged for a remote pull, but that whether this
should be presented to the user alongside `docs/features/remote-deploys/01_ideas.md`'s own
undecided "does `deploy` stream base-image build progress" question is unresolved. A future
iteration could give `deploy` one unified progress channel covering base-image build, mitmproxy image
pull, and remote proxy creation as a single multi-step sequence, rather than two independently
reported activities. Deferred because it depends on `remote-deploys/01_ideas.md`'s own open question
being resolved first, which this document does not control.

## A live two-machine spike against a real remote host

`01_ideas.md`'s entire Validation section is a placeholder for work this feature could not do: no
real non-WSL remote Podman host was available to spike against. Running that spike — ideally on one
of the free-tier cloud VM candidates `docs/features/cloud-sandboxes/01_ideas.md` already identifies
(Oracle Cloud's Always Free A1, or Google Cloud's `e2-micro`) once that feature's own onboarding
panel exists, or by hand against a manually provisioned VM sooner — is the single highest-value
follow-up to this document, and is what unblocks `02_dev.md`'s Phase E. Not recorded as an Open
Question in `01_ideas.md` because it is not a design decision to make; it is a concrete task for
whichever future agent has access to real remote infrastructure.

## Switching an already-deployed sandbox's proxy toggle

Both `01_ideas.md` and `02_dev.md` assume a sandbox's proxy-enabled-ness is decided at local `create`
time and simply follows the sandbox through `deploy`/`undeploy`. Turning inspection on or off for a
sandbox that is *already* deployed (without first undeploying it back to local) is not covered —
`docs/features/network-proxy/02_dev.md`'s own Cross-Cutting Concerns already establishes that toggling
proxy state means recreating the sandbox container even in the local-only case, and doing that for a
remote container mid-deploy is a materially larger change (recreating a remote container, re-running
CA injection, likely a brief connectivity gap) than this document scopes. Deferred until the
local-only toggle-by-recreation feature (`docs/features/network-proxy/04_improvements.md`, "Switching
an existing sandbox between proxied and unproxied") is itself built, since a remote version would
extend that mechanism rather than invent its own.
