import GreetPage from "../pageobjects/greet.page.js";

describe("Greet", () => {
  it("greets MIO and shows the response", async () => {
    await GreetPage.greet("MIO");

    await expect(GreetPage.response).toHaveText(
      "Hello, MIO! You've been greeted from Rust!"
    );
  });
});
