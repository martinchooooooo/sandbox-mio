// pi extension for the standard chat spec: `/spikeconfirm` raises a blocking confirm dialog, then
// notifies the answer. Copied from docs/features/native-agent-ui/spike/spike-confirm.ts.
export default function (pi: any) {
  pi.registerCommand("spikeconfirm", {
    description: "Spike: raise a confirm dialog",
    handler: async (_args: string, ctx: any) => {
      const ok = await ctx.ui.confirm("Spike", "Proceed?");
      ctx.ui.notify(`spikeconfirm answered: ${ok}`, "info");
    },
  });
}
