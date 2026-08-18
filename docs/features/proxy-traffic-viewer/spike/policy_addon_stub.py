# Spike script (Phase A, docs/features/proxy-traffic-viewer/02_dev.md).
#
# Throwaway stand-in for a future src-tauri/resources/mitm_policy_addon.py
# (docs/features/proxy-network-policy). Only needs to exist and hook something of its own for this
# spike's multi-script composition check -- hooks request() and logs, no actual policy logic.

class PolicyAddonStub:
    def request(self, flow):
        print(f"HOOKLOG policy_addon_stub.request path={flow.request.path}", flush=True)


addons = [PolicyAddonStub()]
