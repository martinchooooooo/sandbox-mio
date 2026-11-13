import AppPage, { type Element } from "./app.page.js";
import { COMMAND, PANEL_RENDER } from "../../lib/timeouts.js";

/**
 * The native agent chat (`src/lib/agent-chat/ChatView.svelte`). Every selector is a `data-*`
 * attribute from the contract in docs/features/native-agent-ui/03_e2e.md, never visible text:
 * the chat's strings are still untranslated literals awaiting the Paraglide pass.
 */
class AgentChatPage extends AppPage {
  root(sandbox: string): Element {
    return $(`[data-agent-chat='${sandbox}']`);
  }

  get input(): Element {
    return $("[data-chat-input]");
  }

  get sendButton(): Element {
    return $("[data-chat-send]");
  }

  get modelButton(): Element {
    return $("[data-chat-model]");
  }

  modelOption(id: string): Element {
    return $(`[data-model-option='${id}']`);
  }

  get thinkingButton(): Element {
    return $("[data-chat-thinking]");
  }

  get contextMeter(): Element {
    return $("[data-chat-context]");
  }

  get sessionsButton(): Element {
    return $("[data-chat-sessions]");
  }

  get newSessionItem(): Element {
    return $("[data-chat-new-session]");
  }

  sessionItem(id: string): Element {
    return $(`[data-session-id='${id}']`);
  }

  get menuButton(): Element {
    return $("[data-chat-menu]");
  }

  get stopAgentItem(): Element {
    return $("[data-chat-stop-agent]");
  }

  get stoppedState(): Element {
    return $("[data-chat-stopped]");
  }

  get startAgentButton(): Element {
    return $("[data-chat-start-agent]");
  }

  get deployedState(): Element {
    return $("[data-chat-deployed]");
  }

  get emptyState(): Element {
    return $("[data-chat-empty]");
  }

  get readonlyBanner(): Element {
    return $("[data-chat-readonly]");
  }

  get takeControlButton(): Element {
    return $("[data-chat-take-control]");
  }

  get inferenceDownBanner(): Element {
    return $("[data-chat-inference-down]");
  }

  /** The chat's switcher tab close button; scoped because each sandbox's group has its own. */
  closeTabButton(sandbox: string): Element {
    return $(`div[data-sandbox='${sandbox}'] [data-close-tab='agentchat']`);
  }

  /** Ids of the entries in the open sessions dropdown. */
  async sessionIds(): Promise<string[]> {
    const ids: string[] = [];
    for (const item of await $$("[data-session-id]").getElements()) {
      ids.push((await item.getAttribute("data-session-id")) ?? "");
    }
    return ids;
  }

  dialog(method: string): Element {
    return $(`[data-dialog='${method}']`);
  }

  get toast(): Element {
    return $("[data-sonner-toast]");
  }

  async attribute(sandbox: string, name: string): Promise<string | null> {
    return (await this.root(sandbox)).getAttribute(name);
  }

  async waitForConnection(sandbox: string, connection: string, timeout = PANEL_RENDER): Promise<void> {
    await browser.waitUntil(async () => (await this.attribute(sandbox, "data-connection")) === connection, {
      timeout,
      timeoutMsg: `chat for ${sandbox} never reached data-connection="${connection}"`,
    });
  }

  async waitForIdle(sandbox: string, timeout = PANEL_RENDER): Promise<void> {
    await browser.waitUntil(async () => (await this.attribute(sandbox, "data-run")) === "idle", {
      timeout,
      timeoutMsg: `chat for ${sandbox} never returned to data-run="idle"`,
    });
  }

  async messageCount(role: "user" | "assistant"): Promise<number> {
    return (await $$(`[data-role='${role}']`)).length;
  }

  async lastAssistantText(): Promise<string> {
    const all = await $$("[data-role='assistant']").getElements();
    return all.length === 0 ? "" : all[all.length - 1].getText();
  }

  async dialogCount(): Promise<number> {
    return (await $$("[data-dialog]")).length;
  }

  async send(text: string): Promise<void> {
    const input = await this.input;
    await input.waitForEnabled({ timeout: COMMAND });
    await input.setValue(text);
    const send = await this.sendButton;
    await send.waitForClickable({ timeout: COMMAND });
    await send.click();
  }

  /** Opens a header dropdown and picks one item; bits-ui renders the items in a portal. */
  async pick(trigger: Element, item: Element): Promise<void> {
    await (await trigger).waitForClickable({ timeout: COMMAND });
    await (await trigger).click();
    await (await item).waitForClickable({ timeout: COMMAND });
    await (await item).click();
  }
}

export default new AgentChatPage();
