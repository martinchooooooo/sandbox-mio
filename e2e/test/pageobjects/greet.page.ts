import AppPage, { type Element } from "./app.page.js";

class GreetPage extends AppPage {
  get input(): Element {
    return $("#greet-input");
  }

  get submitButton(): Element {
    return $("button[type=submit]");
  }

  get response(): Element {
    return $("p");
  }

  async greet(name: string): Promise<void> {
    await (await this.input).setValue(name);
    await (await this.submitButton).click();
  }
}

export default new GreetPage();
