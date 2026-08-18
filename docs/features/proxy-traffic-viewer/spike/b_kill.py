# Spike script (Phase A, docs/features/proxy-traffic-viewer/02_dev.md).
#
# Plays the role of the future mitm_flowlog_addon.py -- loaded second (-s a_kill.py before
# -s b_kill.py), hooks BOTH response() and error() (unlike b_synth.py, which only needs response())
# so this run can tell which hook, if either, fires for a flow a_kill.py killed via flow.kill().

class BKill:
    def response(self, flow):
        print(f"HOOKLOG b_kill.response path={flow.request.path}", flush=True)

    def error(self, flow):
        print(f"HOOKLOG b_kill.error path={flow.request.path} error={flow.error}", flush=True)


addons = [BKill()]
