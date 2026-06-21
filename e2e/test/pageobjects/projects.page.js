import AppPage from "./app.page.js";
import ProjectRow from "./project-row.component.js";

// The projects screen: reached via the nav link, creates projects and hands out a ProjectRow
// (see project-row.component.js) for each one in the list.
class ProjectsPage extends AppPage {
  get navLink() {
    return $("a[href='/projects']");
  }

  get nameInput() {
    return $("input[placeholder='my-project']");
  }

  get createButton() {
    return $("button=Create");
  }

  // Navigates to the projects screen; returns once the nav link has been clicked.
  async open(timeout = 10000) {
    const link = await this.navLink;
    await link.waitForDisplayed({ timeout });
    await link.click();
  }

  // Creates the project and returns its row once the row is displayed.
  async createProject(name) {
    await (await this.nameInput).setValue(name);
    await (await this.createButton).click();

    const row = this.row(name);
    await row.waitForDisplayed();
    return row;
  }

  // Returns a row component for the named project without asserting it exists yet.
  row(name) {
    return new ProjectRow(name);
  }
}

export default new ProjectsPage();
