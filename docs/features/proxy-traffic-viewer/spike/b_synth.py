# Spike script (Phase A, docs/features/proxy-traffic-viewer/02_dev.md).
#
# Plays the role of the future mitm_flowlog_addon.py -- loaded second (-s a_synth.py before
# -s b_synth.py), hooks response() only, and logs whether it observed the synthetic response
# a_synth.py's request() hook set (status code + the distinctive header), confirming a flow-log
# addon reaches response() for a flow that was resolved via flow.response rather than flow.kill().

class BSynth:
    def response(self, flow):
        status = flow.response.status_code if flow.response else None
        header = flow.response.headers.get("X-Synth-Blocked") if flow.response else None
        print(
            f"HOOKLOG b_synth.response path={flow.request.path} status={status} "
            f"synth_header={header}",
            flush=True,
        )


addons = [BSynth()]
