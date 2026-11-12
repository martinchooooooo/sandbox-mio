// Spike extension: `/spikeconfirm` raises a blocking confirm dialog, then notifies the answer.
// Provisioned into <PI_CODING_AGENT_DIR>/extensions/ so pi auto-discovers it as a global extension.
export default function (pi: any) {
  pi.registerCommand("spikeconfirm", {
    description: "Spike: raise a confirm dialog",
    handler: async (_args: string, ctx: any) => {
      const ok = await ctx.ui.confirm("Spike", "Proceed?");
      ctx.ui.notify(`spikeconfirm answered: ${ok}`, "info");
    },
  });
}
