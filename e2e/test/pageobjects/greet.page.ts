import AppPage, { type Element } from "./app.page.js";

// The greet screen: a text input, a submit button and the response paragraph.
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
