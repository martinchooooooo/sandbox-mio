import type { AgentDescriptor } from "./types";

export const AGENTS: AgentDescriptor[] = [
  {
    id: "claude-code",
    label: "Claude Code",
    description: "Anthropic's AI coding agent",
    misePackages: [
      "node@lts",
      "npm:@anthropic-ai/claude-code@2.1.195",
    ],
    postInstallSteps: [
      {
        kind: "npm-postinstall",
        package: "@anthropic-ai/claude-code",
        nodeVersion: "node@lts",
      },
    ],
    auth: {
      kind: "exec-interactive",
      command: ["claude", "auth", "login"],
      credentialFile: "/root/.claude/.credentials.json",
      expiresAtJsonKey: "claudeAiOauth.expiresAt",
      accountSourceFile: "/root/.claude.json",
      accountEmailJsonKey: "oauthAccount.emailAddress",
    },
  },
  {
    id: "opencode",
    label: "OpenCode",
    description: "Open-source terminal coding agent, pre-wired to your local LLM",
    misePackages: ["opencode"],
    postInstallSteps: [],
    // No `auth`: OpenCode configures a provider instead of authenticating.
    provision: { kind: "opencode-provider", providerId: "local-llm", relativeConfigPath: "opencode.json" },
  },
  {
    id: "pi",
    label: "Pi",
    description: "Minimal terminal coding agent, pre-wired to your local LLM",
    misePackages: ["pi"],
    postInstallSteps: [],
    // No `auth`: pi configures a provider instead of authenticating. The nested config path is
    // redirected into the workspace via PI_CODING_AGENT_DIR (set by provision_pi in mise.toml).
    provision: { kind: "pi-provider", providerId: "local-llm", relativeConfigPath: ".mio/pi-agent/models.json" },
  },
];

/** Extract the mise.toml key from a full package spec (strips the @version suffix). */
export function miseKey(spec: string): string {
  const colonIdx = spec.indexOf(":");
  if (colonIdx === -1) {
    const atIdx = spec.indexOf("@");
    return atIdx === -1 ? spec : spec.slice(0, atIdx);
  }
  // "npm:@anthropic-ai/claude-code@2.1.195" → "npm:@anthropic-ai/claude-code"
  const afterScheme = spec.slice(colonIdx + 1);
  const lastAt = afterScheme.lastIndexOf("@");
  if (lastAt <= 0) return spec;
  return spec.slice(0, colonIdx + 1 + lastAt);
}
