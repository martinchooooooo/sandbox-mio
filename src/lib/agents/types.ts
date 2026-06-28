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

export interface AgentDescriptor {
  id: string;
  label: string;
  description: string;
  misePackages: string[];
  postInstallSteps: PostInstallStep[];
  auth: AuthStrategy;
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
