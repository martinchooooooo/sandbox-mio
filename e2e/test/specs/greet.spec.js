describe("Greet", () => {
  it("greets MIO and shows the response", async () => {
    const input = await $("#greet-input");
    await input.setValue("MIO");

    const submitBtn = await $("button[type=submit]");
    await submitBtn.click();

    const msg = await $("p");
    await expect(msg).toHaveText("Hello, MIO! You've been greeted from Rust!");
  });
});
