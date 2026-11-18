// Step 3.5: the design system (D-053) on the Studio's playground. Accessibility in light and dark,
// with and without the system's "more contrast"; keyboard behaviour; the appearance choice; and
// no requests to anyone else's servers.

import AxeBuilder from "@axe-core/playwright";
import { expect, type Page, test } from "@playwright/test";
import { openFixture } from "./helpers.ts";

/** The theme toggle: one button, named "Dark theme", pressed when dark is on. */
const themeToggle = (page: Page) =>
  page.getByRole("button", { name: "Dark theme" });

/** Turns `mode` on with the toggle, pressing it only if the other theme is showing. */
async function choose(page: Page, mode: "Light" | "Dark") {
  const toggle = themeToggle(page);
  const pressed = mode === "Dark" ? "true" : "false";
  if ((await toggle.getAttribute("aria-pressed")) !== pressed) await toggle.click();
  await expect(toggle).toHaveAttribute("aria-pressed", pressed);
}

const storedChoice = (page: Page) =>
  page.evaluate(() => localStorage.getItem("mode-watcher-mode"));

async function axe(page: Page) {
  const results = await new AxeBuilder({ page })
    .withTags(["wcag2a", "wcag2aa", "wcag21a", "wcag21aa", "wcag22aa"])
    .analyze();
  return results.violations.map(
    (violation) =>
      `${violation.id}: ${violation.help} (${violation.nodes.map((node) => node.target.join(" ")).join(", ")})`,
  );
}

const token = (page: Page, name: string) =>
  page.evaluate(
    (property) =>
      getComputedStyle(document.documentElement)
        .getPropertyValue(property)
        .trim(),
    name,
  );

for (const mode of ["Light", "Dark"] as const) {
  for (const contrast of ["no-preference", "more"] as const) {
    test(`no accessibility violations: ${mode.toLowerCase()}, ${contrast === "more" ? "more" : "standard"} contrast`, async ({
      page,
    }) => {
      await page.emulateMedia({ contrast });
      await page.goto("/studio/playground");
      await choose(page, mode);
      await expect(page.locator("html")).toHaveClass(
        mode === "Dark" ? /\bdark\b/ : /^(?!.*\bdark\b)/,
      );
      await openFixture(page, "valid/12-blog-post");
      expect(await axe(page), "editor").toEqual([]);
      await page.getByRole("tab", { name: "Preview" }).click();
      expect(await axe(page), "preview").toEqual([]);
      // The image dialog needs a project's media library, which the playground has none of:
      // tests/server/rich-text-images.spec.ts checks it in both themes.
    });
  }
}

test("the formatting toolbar is one tab stop, with arrow keys inside it that wrap", async ({
  page,
}) => {
  await page.goto("/studio/playground");
  await page
    .getByRole("toolbar", { name: "Formatting" })
    .getByRole("button")
    .first()
    .focus();
  await expect(page.getByTitle("Bold (⌘B)")).toBeFocused();
  await page.keyboard.press("ArrowRight");
  await expect(page.getByTitle("Italic (⌘I)")).toBeFocused();
  await page.keyboard.press("ArrowLeft");
  await page.keyboard.press("ArrowLeft");
  await expect(page.getByTitle("Redo (⌘⇧Z)")).toBeFocused();
  // Only one button in the toolbar is in the tab order.
  const tabbable = await page
    .getByRole("toolbar", { name: "Formatting" })
    .locator('button[tabindex="0"]')
    .count();
  expect(tabbable).toBe(1);
});

test.describe("with motion", () => {
  test.use({ reducedMotion: "no-preference" });
  test("the theme follows the system until toggled, flips the colours (through the view transition), survives a reload, and goes back to the system when toggled onto its scheme", async ({
    page,
  }) => {
    const html = page.locator("html");
    await page.emulateMedia({ colorScheme: "light" });
    await page.goto("/studio/playground");
    // Nobody has chosen: the Studio follows the system, both ways.
    await expect(html).not.toHaveClass(/\bdark\b/);
    await expect(themeToggle(page)).toHaveAttribute("aria-pressed", "false");
    const light = await token(page, "--background");
    await page.emulateMedia({ colorScheme: "dark" });
    await expect(html).toHaveClass(/\bdark\b/);
    await expect(themeToggle(page)).toHaveAttribute("aria-pressed", "true");
    await page.emulateMedia({ colorScheme: "light" });
    await expect(html).not.toHaveClass(/\bdark\b/);

    // One press turns dark on and keeps it, whatever the system says, across a reload.
    await themeToggle(page).click();
    await expect(html).toHaveClass(/\bdark\b/);
    await expect(themeToggle(page)).toHaveAttribute("aria-pressed", "true");
    const dark = await token(page, "--background");
    expect(dark).not.toBe(light);
    expect(await storedChoice(page)).toBe("dark");
    await page.reload();
    await expect(html).toHaveClass(/\bdark\b/);
    expect(await token(page, "--background")).toBe(dark);

    // Pressing it onto the system's own scheme goes back to following the system.
    await themeToggle(page).click();
    await expect(html).not.toHaveClass(/\bdark\b/);
    expect(await storedChoice(page)).toBe("system");
    await page.emulateMedia({ colorScheme: "dark" });
    await expect(html).toHaveClass(/\bdark\b/);

    // Against the system, a press is a choice of its own: dark system, light Studio.
    await themeToggle(page).click();
    await expect(html).not.toHaveClass(/\bdark\b/);
    expect(await storedChoice(page)).toBe("light");
  });
});

test("asking the system for more contrast darkens secondary text without a setting", async ({
  page,
}) => {
  await page.goto("/studio/playground");
  await choose(page, "Light");
  const standard = await token(page, "--muted-foreground");
  await page.emulateMedia({ contrast: "more" });
  expect(await token(page, "--muted-foreground")).not.toBe(standard);
});

test("the Studio makes no requests to anyone else: the font is self-hosted", async ({
  page,
  baseURL,
}) => {
  const foreign: string[] = [];
  page.on("request", (request) => {
    if (
      !request.url().startsWith(baseURL ?? "") &&
      !request.url().startsWith("data:")
    )
      foreign.push(request.url());
  });
  await page.goto("/studio/playground");
  await openFixture(page, "valid/12-blog-post");
  await page.getByRole("tab", { name: "Preview" }).click();
  await page.waitForLoadState("networkidle");
  expect(foreign).toEqual([]);
  expect(
    await page.evaluate(() => getComputedStyle(document.body).fontFamily),
  ).toContain("Inter Variable");
  // One typeface for everything people read, titles included.
  expect(
    await page.evaluate(
      () =>
        getComputedStyle(document.querySelector("h1") ?? document.body)
          .fontFamily,
    ),
  ).toContain("Inter Variable");
});
