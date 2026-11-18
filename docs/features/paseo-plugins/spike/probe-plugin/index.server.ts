import type { PluginServerContext } from "@getpaseo/plugin/server";

const MARK = "probe-v1";

export default function contribute(server: PluginServerContext) {
  console.log(`mio-probe server started: ${MARK}`);
  return () => {
    console.log(`mio-probe server stopped: ${MARK}`);
  };
}
