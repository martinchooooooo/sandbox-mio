import AppPage, { type Element } from "./app.page.js";
import { COMMAND } from "../../lib/timeouts.js";

// Observes background tasks on any route; ModelsPage is what starts them. Selectors follow the
// Selector Contract in docs/features/activity-center/03_e2e.md.
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

  /** `id` is the stable task id, e.g. "model-download:smollm2-135m". */
  taskRow(id: string): Element {
    return $(`[data-activity-task='${id}']`);
  }

  get toast(): Element {
    return $("[data-sonner-toast]");
  }

  async isToggleShown(): Promise<boolean> {
    return (await this.toggle).isExisting();
  }

  async taskCount(): Promise<number> {
    return (await $$("[data-activity-task]")).length;
  }

  /**
   * Re-open after every refresh or nav: the open state is component-local. Opening also markSeen()s
   * the store, zeroing the unseen-failure badge.
   */
  async openPanel(timeout = COMMAND): Promise<void> {
    const t = await this.toggle;
    await t.waitForClickable({ timeout });
    if (!(await (await this.panel).isExisting())) {
      await t.click();
    }
    await (await this.panel).waitForDisplayed({ timeout });
  }

  /** 0 when the badge is absent — no running tasks and no unseen failures. */
  async badgeCount(): Promise<number> {
    const b = await this.badge;
    if (!(await b.isExisting())) return 0;
    return parseInt((await b.getText()).trim(), 10) || 0;
  }

  /** Pass several statuses when a task may go terminal before the first poll. */
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

  /** The verbatim backend detail line: progress text, base_url, or error. */
  async taskDetail(id: string): Promise<string> {
    const el = await (await this.taskRow(id)).$("[data-task-detail]");
    await el.waitForExist({ timeout: COMMAND });
    return (await el.getText()).trim();
  }

  async eventsText(): Promise<string> {
    return (await this.events).getText();
  }

  /** Best-effort: success toasts auto-dismiss, so null means "none rendered right now". */
  async toastType(): Promise<string | null> {
    const t = await this.toast;
    if (!(await t.isExisting())) return null;
    return t.getAttribute("data-type");
  }
}

export default new ActivityCenter();
