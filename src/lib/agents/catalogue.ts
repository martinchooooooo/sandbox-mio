import type { AgentDescriptor, OnboardingSeed } from "./types";
import { PI_PINNED_VERSION } from "./pi";

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
      onboarding: {
        workspaceDir: "/root/work",
        accountFilePatch: { hasCompletedOnboarding: true },
        projectTrustKeys: ["hasTrustDialogAccepted"],
      },
    },
    credentialService: "anthropic",
    apiHosts: ["api.anthropic.com", "console.anthropic.com", "claude.ai", "statsig.anthropic.com"],
  },
  {
    id: "opencode",
    label: "OpenCode",
    description: "Open-source terminal coding agent, pre-wired to your local LLM",
    misePackages: ["opencode"],
    postInstallSteps: [],
    provision: { kind: "opencode-provider", providerId: "local-llm", relativeConfigPath: "opencode.json" },
    apiHosts: ["models.dev", "opencode.ai"],
  },
  {
    id: "pi",
    label: "Pi",
    description: "Minimal terminal coding agent, pre-wired to your local LLM",
    misePackages: [`pi@${PI_PINNED_VERSION}`],
    postInstallSteps: [],
    provision: { kind: "pi-provider", providerId: "local-llm", relativeConfigPath: ".mio/pi-agent/models.json" },
    // The local LLM is reached through the reverse forward, which is not egress.
    apiHosts: [],
  },
];

/** Every catalogue agent's API hosts, deduplicated — installed or not (Resolved Decision 13). */
export function allAgentApiHosts(): string[] {
  return [...new Set(AGENTS.flatMap((a) => a.apiHosts))];
}

export function onboardingSeed(agent: AgentDescriptor): OnboardingSeed | null {
  const auth = agent.auth;
  if (!auth?.onboarding || !auth.accountSourceFile) return null;
  return {
    accountSourcePath: auth.accountSourceFile,
    workspaceDir: auth.onboarding.workspaceDir,
    accountFilePatch: auth.onboarding.accountFilePatch,
    projectTrustKeys: auth.onboarding.projectTrustKeys,
  };
}

export function miseKey(spec: string): string {
  const colonIdx = spec.indexOf(":");
  if (colonIdx === -1) {
    const atIdx = spec.indexOf("@");
    return atIdx === -1 ? spec : spec.slice(0, atIdx);
  }
  const afterScheme = spec.slice(colonIdx + 1);
  const lastAt = afterScheme.lastIndexOf("@");
  if (lastAt <= 0) return spec;
  return spec.slice(0, colonIdx + 1 + lastAt);
}
