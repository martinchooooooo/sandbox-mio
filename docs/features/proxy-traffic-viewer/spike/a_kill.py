# Spike script (Phase A, docs/features/proxy-traffic-viewer/02_dev.md).
#
# Plays the role of a future policy addon that blocks via flow.kill() (the alternative mechanism to
# a_synth.py's flow.response approach) -- loaded first (-s a_kill.py before -s b_kill.py). For path
# "/kill" it calls flow.kill() in request(), to confirm whether a flow-log addon's response()/error()
# hooks ever see a killed flow at all.

class AKill:
    def request(self, flow):
        print(f"HOOKLOG a_kill.request path={flow.request.path}", flush=True)
        if flow.request.path == "/kill":
            flow.kill()


addons = [AKill()]
