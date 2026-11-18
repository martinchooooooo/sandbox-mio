import type { PluginClientContext } from "@getpaseo/plugin/client";

export default function contribute(client: PluginClientContext) {
  client.addCommandCenterItem({
    id: "mio-probe",
    title: "Sandbox-mio probe v1",
    icon: "FlaskConical",
    context: "global",
    onSelect() {},
  });
  return () => {};
}
