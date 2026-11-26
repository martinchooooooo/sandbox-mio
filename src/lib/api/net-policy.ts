import { invoke } from "@tauri-apps/api/core";
import type { ApplyReport, NetPolicy, NetPolicyState } from "$lib/net-policy/net-policy";

export function netPolicyState(name: string): Promise<NetPolicyState> {
    return invoke<NetPolicyState>("net_policy_state", { name });
}

export interface ApplySandboxPolicyArgs {
    name: string;
    policy: NetPolicy;
    /** The `revision` the caller's edit started from; a stale one is rejected. */
    baseRevision: number;
    /** Stored only when the new policy applies. */
    storeSecret?: { secretId: string; value: string };
    dropSecretId?: string;
}

export function applySandboxPolicy(args: ApplySandboxPolicyArgs): Promise<ApplyReport> {
    return invoke<ApplyReport>("apply_sandbox_policy", { ...args });
}

export function discardNetPolicyProposal(name: string): Promise<void> {
    return invoke("discard_net_policy_proposal", { name });
}
