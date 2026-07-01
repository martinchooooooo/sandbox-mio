export interface NpmPostInstallStep {
  kind: "npm-postinstall";
  package: string;
  nodeVersion: string;
}

export type PostInstallStep = NpmPostInstallStep;

export interface ExecInteractiveAuth {
  kind: "exec-interactive";
  command: string[];
  credentialFile: string;
  expiresAtJsonKey: string;
  accountSourceFile?: string;
  accountEmailJsonKey?: string;
}

export type AuthStrategy = ExecInteractiveAuth;

/** Provider-provisioning step for agents that configure an endpoint instead of authenticating.
 * The concrete provider JSON and default model are generated at call time from the model
 * catalogue (see `buildLocalLlmProvider`), so the catalogue stays the single source of truth. */
export interface ProviderProvisionConfig {
  kind: "opencode-provider" | "pi-provider"; // discriminant selecting the provisioner
  providerId: string; // "local-llm"
  relativeConfigPath: string; // config file relative to the workspace root (e.g. "opencode.json")
}

export interface AgentDescriptor {
  id: string;
  label: string;
  description: string;
  misePackages: string[];
  postInstallSteps: PostInstallStep[];
  /** Absent for agents that configure a provider instead of authenticating (e.g. OpenCode). */
  auth?: AuthStrategy;
  provision?: ProviderProvisionConfig;
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
