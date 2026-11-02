export interface NpmPostInstallStep {
  kind: "npm-postinstall";
  package: string;
  nodeVersion: string;
}

export type PostInstallStep = NpmPostInstallStep;

export interface OnboardingSeedConfig {
  workspaceDir: string;
  accountFilePatch: Record<string, unknown>;
  projectTrustKeys: string[];
}

export interface ExecInteractiveAuth {
  kind: "exec-interactive";
  command: string[];
  credentialFile: string;
  expiresAtJsonKey: string;
  accountSourceFile?: string;
  accountEmailJsonKey?: string;
  onboarding?: OnboardingSeedConfig;
}

export type AuthStrategy = ExecInteractiveAuth;

export interface OnboardingSeed {
  accountSourcePath: string;
  workspaceDir: string;
  accountFilePatch: Record<string, unknown>;
  projectTrustKeys: string[];
}

export interface ProviderProvisionConfig {
  kind: "opencode-provider" | "pi-provider";
  providerId: string;
  relativeConfigPath: string;
}

export interface AgentDescriptor {
  id: string;
  label: string;
  description: string;
  misePackages: string[];
  postInstallSteps: PostInstallStep[];
  auth?: AuthStrategy;
  provision?: ProviderProvisionConfig;
  credentialService?: string;
  /**
   * Hosts the agent itself must reach, seeded into a new allowlist network policy
   * (docs/features/microsandbox-network-policy/02_dev.md, Resolved Decision 13).
   */
  apiHosts: string[];
}

export type AgentInstallEvent =
  | { kind: "PackageStart"; package: string }
  | { kind: "PackageDone"; package: string }
  | { kind: "PostInstallStart"; step: string }
  | { kind: "PostInstallDone"; step: string }
  | { kind: "Done" };

export type AgentAuthEvent =
  | { kind: "Output"; line: string }
  | { kind: "Exit"; code: number };

export interface AgentAuthStatus {
  authenticated: boolean;
  expiresAtMs: number | null;
  accountEmail: string | null;
}
