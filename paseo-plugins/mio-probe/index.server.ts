import { hostname } from "node:os";
import type { PluginServerContext } from "@getpaseo/plugin/server";
import { probeInfo } from "./shared/probe";

export default function contribute(server: PluginServerContext) {
  const startedAt = new Date().toISOString();
  server.handle(probeInfo, async () => ({ hostname: hostname(), node: process.version, startedAt }));
  return () => {};
}
