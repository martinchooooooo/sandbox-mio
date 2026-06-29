import AppPage, { type Element } from "./app.page.js";

// The global activity center, mounted once in src/routes/+layout.svelte so it is present on every
// route. This page object *observes* background tasks globally; ModelsPage *starts* the work.
// Selectors are the data-attribute Selector Contract from docs/features/activity-center/03_e2e.md
// (set in src/lib/activity/ActivityCenter.svelte) so it never depends on CSS classes or copy.
class ActivityCenter extends AppPage {
  get toggle(): Element {
    return $("[data-activity-toggle]");
  }

  get panel(): Element {
    return $("[data-activity-panel]");
  }

  get badge(): Element {
    return $("[data-activity-badge]");
  }

  get events(): Element {
    return $("[data-activity-events]");
  }

  // The row for a task, keyed by its stable task id (e.g. "model-download:smollm2-135m").
  taskRow(id: string): Element {
    return $(`[data-activity-task='${id}']`);
  }

  get toast(): Element {
    return $("[data-sonner-toast]");
  }

  // Whether the bell toggle is rendered on the current route (it lives in the root layout, so it
  // must exist everywhere). Does not open the panel.
  async isToggleShown(): Promise<boolean> {
    return (await this.toggle).isExisting();
  }

  // Number of task rows currently rendered in the panel (0 when idle).
  async taskCount(): Promise<number> {
    return (await $$("[data-activity-task]")).length;
  }

  // Open the panel from any route. The panel's open/closed state is component-local UI (reset by a
  // reload/navigation), so callers re-open after every browser.refresh() or nav. Opening also calls
  // markSeen() in the store, zeroing the unseen-failure badge.
  async openPanel(timeout = 10000): Promise<void> {
    const t = await this.toggle;
    await t.waitForClickable({ timeout });
    if (!(await (await this.panel).isExisting())) {
      await t.click();
    }
    await (await this.panel).waitForDisplayed({ timeout });
  }

  // The badge count; 0 when the badge element is absent (no running tasks / unseen failures).
  async badgeCount(): Promise<number> {
    const b = await this.badge;
    if (!(await b.isExisting())) return 0;
    return parseInt((await b.getText()).trim(), 10) || 0;
  }

  // Poll until the task row exists with the given status. Accepts a single status or several (a
  // fast download may already be terminal before the first poll, so callers pass running|succeeded).
  async waitForTask(id: string, status: string | string[], timeoutMs: number): Promise<void> {
    const wanted = Array.isArray(status) ? status : [status];
    const row = this.taskRow(id);
    await browser.waitUntil(
      async () => {
        if (!(await row.isExisting())) return false;
        const s = await row.getAttribute("data-task-status");
        return s !== null && wanted.includes(s);
      },
      {
        timeout: timeoutMs,
        interval: 1000,
        timeoutMsg: `task ${id} did not reach [${wanted.join("|")}] within ${timeoutMs}ms`,
      },
    );
  }

  // The verbatim backend detail line for a task row (progress text, base_url, or error).
  async taskDetail(id: string): Promise<string> {
    const el = await (await this.taskRow(id)).$("[data-task-detail]");
    await el.waitForExist({ timeout: 10000 });
    return (await el.getText()).trim();
  }

  async eventsText(): Promise<string> {
    return (await this.events).getText();
  }

  // The latest toast's type ("success"/"error") read from the sonner data-type attribute, or null
  // when no toast is currently rendered (success toasts auto-dismiss, so this is best-effort).
  async toastType(): Promise<string | null> {
    const t = await this.toast;
    if (!(await t.isExisting())) return null;
    return t.getAttribute("data-type");
  }
}

export default new ActivityCenter();
