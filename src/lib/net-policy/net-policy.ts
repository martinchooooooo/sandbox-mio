// Mirrors Rust `net_policy::NetPolicy` (camelCase, `.mio/net-policy.json` on disk).
export interface NetRule {
    action: "allow" | "deny";
    host?: string;
    cidr?: string;
    group?: "public" | "private" | "host";
    protocol?: "tcp" | "udp";
}
export interface SecretBinding {
    secretId: string;
    envVar: string;
    hosts: string[];
    headers: string[];
    query: boolean;
    body: boolean;
}
export interface NetPolicy {
    version: number;
    egress: "open" | "allowlist";
    rules: NetRule[];
    strictHostnames: boolean;
    secrets: SecretBinding[];
    interactive?: boolean;
}

// Mirrors Rust `net_policy::UnstampedBinding`.
export interface UnstampedBinding {
    envVar: string;
    placeholder: string;
}

// Mirrors Rust `net_policy::PolicyChange`: `kind` is the `ChangeDetail` tag, `text` the summary.
export interface PolicyChange {
    kind: string;
    text: string;
    widens: boolean;
    envVar?: string;
    host?: string;
    hosts?: string[];
    /** Built by the backend: which hosts receive which stored secret's real value. */
    risk?: string;
}

// Mirrors Rust `commands::net_policy::{Proposal, NetPolicyState}`.
export interface Proposal {
    policy: NetPolicy;
    changes: PolicyChange[];
    unstamped: UnstampedBinding[];
}
export interface NetPolicyState {
    accepted: NetPolicy;
    revision: number;
    unstamped: UnstampedBinding[];
    proposal: Proposal | null;
    proposalError: string | null;
}

// Mirrors Rust `proxy::apply::ApplyReport`.
export interface ApplyReport {
    missingSecrets: string[];
}
