# Spike script (Phase A1, docs/features/proxy-network-policy/02_dev.md).
#
# Plays the role of the future mitm_addon.py / SecretInjector: loaded second (-s a.py before
# -s b.py), unconditionally mutates flow.request.headers on every request() call -- no
# flow.response guard, deliberately, to observe what happens when a.py has already blocked
# the flow. Confirms whether this mutation has any client-visible effect once flow.response
# is already set by an earlier script's same-named hook.

class B:
    def request(self, flow):
        print(f"HOOKLOG b.request path={flow.request.path} flow.response is None={flow.response is None}", flush=True)
        flow.request.headers["X-Secret-Injected"] = "should-not-leak-when-blocked"


addons = [B()]
