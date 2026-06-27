import { parseError } from "$lib/errors";

export interface ActionRunner {
    readonly busy: string | null;
    readonly message: string;
    readonly error: string;
    run(key: string, thunk: () => Promise<unknown>, opts?: { message?: string }): Promise<boolean>;
}

export function createActionRunner(): ActionRunner {
    let busy = $state<string | null>(null);
    let message = $state("");
    let error = $state("");

    return {
        get busy() { return busy; },
        get message() { return message; },
        get error() { return error; },
        async run(key, thunk, opts) {
            error = "";
            busy = key;
            message = opts?.message ?? "";
            try {
                await thunk();
                return true;
            } catch (e) {
                error = parseError(e).message;
                return false;
            } finally {
                busy = null;
                message = "";
            }
        },
    };
}
