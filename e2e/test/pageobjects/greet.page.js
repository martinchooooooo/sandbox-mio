import AppPage from "./app.page.js";

// The greet screen: a text input, a submit button and the response paragraph.
class GreetPage extends AppPage {
  get input() {
    return $("#greet-input");
  }

  get submitButton() {
    return $("button[type=submit]");
  }

  get response() {
    return $("p");
  }

  async greet(name) {
    await (await this.input).setValue(name);
    await (await this.submitButton).click();
  }
}

export default new GreetPage();
