import type { AgentDescriptor, OnboardingSeed } from "./types";

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
  },
  {
    id: "opencode",
    label: "OpenCode",
    description: "Open-source terminal coding agent, pre-wired to your local LLM",
    misePackages: ["opencode"],
    postInstallSteps: [],
    provision: { kind: "opencode-provider", providerId: "local-llm", relativeConfigPath: "opencode.json" },
  },
  {
    id: "pi",
    label: "Pi",
    description: "Minimal terminal coding agent, pre-wired to your local LLM",
    misePackages: ["pi"],
    postInstallSteps: [],
    provision: { kind: "pi-provider", providerId: "local-llm", relativeConfigPath: ".mio/pi-agent/models.json" },
  },
];

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
