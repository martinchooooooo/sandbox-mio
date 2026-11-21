import {
  type PluginClientContext,
  type PluginWorkspacePanelProps,
  useRpc,
  useWorkspace,
} from "@getpaseo/plugin/client";
import { useQuery } from "@tanstack/react-query";
import { useMemo } from "react";
import { Text, View } from "react-native";
import { probeInfo } from "./shared/probe";

function ProbePanel({ theme, layout, workspaceId }: PluginWorkspacePanelProps) {
  const name = useWorkspace(workspaceId, (workspace) => workspace.name);
  const callProbe = useRpc(probeInfo);
  const info = useQuery({ queryKey: ["mio-probe", "info"], queryFn: () => callProbe({}) });
  const styles = useMemo(
    () => ({
      screen: {
        flex: 1,
        padding: layout.compact ? 16 : 24,
        gap: layout.compact ? 8 : 12,
        backgroundColor: theme.colors.surface0,
      },
      title: { color: theme.colors.foreground, fontSize: layout.compact ? 20 : 24 },
      line: { color: theme.colors.foregroundMuted },
    }),
    [theme, layout.compact],
  );
  return (
    <View style={styles.screen}>
      <Text style={styles.title}>Sandbox-mio probe: {name}</Text>
      {info.data ? (
        <>
          <Text style={styles.line}>Daemon host: {info.data.hostname}</Text>
          <Text style={styles.line}>Node: {info.data.node}</Text>
          <Text style={styles.line}>Server started: {info.data.startedAt}</Text>
        </>
      ) : (
        <Text style={styles.line}>{info.error ? `RPC failed: ${String(info.error)}` : "Asking the daemon…"}</Text>
      )}
    </View>
  );
}

export default function contribute(client: PluginClientContext) {
  client.addWorkspacePanel({
    id: "probe",
    title: "Sandbox-mio probe",
    icon: "FlaskConical",
    context: "workspace",
    Component: ProbePanel,
  });
  client.addCommandCenterItem({
    id: "open-probe",
    title: "Open Sandbox-mio probe",
    icon: "FlaskConical",
    keywords: ["mio-probe", "probe", "sandbox-mio"],
    context: "workspace",
    onSelect({ openPanel }) {
      openPanel("probe");
    },
  });
  return () => {};
}
