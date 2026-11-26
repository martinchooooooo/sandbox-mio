import { getSettings, logMilestone, setSettings } from "$lib/api/app";

// Webview start; the fallback zero point when the persisted first-open time cannot be read.
const webviewStart = Date.now();
const seen = new Set<string>();
let origin: Promise<number> | null = null;

/** First-run milestones, in order; a name is the log field the time-to-wow metric is read from. */
export type Milestone =
    | "app-first-open"
    | "heads-up-confirmed"
    | "model-downloaded"
    | "server-started"
    | "sandbox-created"
    | "pi-installed";

function log(name: Milestone, elapsedMs: number): void {
    logMilestone(name, elapsedMs).catch(() => {});
}

// The settings file keeps the first-open time, so a restart mid-onboarding does not reset the clock.
function firstOpenTime(): Promise<number> {
    origin ??= (async () => {
        try {
            const settings = await getSettings();
            if (typeof settings.firstOpenedAtMs === "number") return settings.firstOpenedAtMs;
            const now = Date.now();
            await setSettings({ ...settings, firstOpenedAtMs: now });
            seen.add("app-first-open");
            log("app-first-open", 0);
            return now;
        } catch {
            return webviewStart;
        }
    })();
    return origin;
}

/** Starts the first-run clock; call once at launch while onboarding is incomplete. Never throws. */
export function startMilestoneClock(): void {
    void firstOpenTime();
}

/** Records `name` once per app run, with the time since the first launch. Never throws. */
export function recordMilestone(name: Milestone): void {
    if (seen.has(name)) return;
    seen.add(name);
    void firstOpenTime().then((start) => log(name, Math.max(0, Date.now() - start)));
}
