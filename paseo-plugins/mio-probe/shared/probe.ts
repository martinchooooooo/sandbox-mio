import { defineRpc } from "@getpaseo/plugin";
import { z } from "zod";

/** Answered by the daemon subprocess, proving the plugin's server half runs in the sandbox. */
export const probeInfo = defineRpc({
  name: "probe.info",
  input: z.object({}),
  output: z.object({ hostname: z.string(), node: z.string(), startedAt: z.string() }),
});
