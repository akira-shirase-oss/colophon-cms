// Step 3.5: the design system (D-053) on the Studio's playground. Accessibility in light and dark,
// with and without the system's "more contrast"; keyboard behaviour; the appearance choice; and
// no requests to anyone else's servers.

import AxeBuilder from "@axe-core/playwright";
import { expect, type Page, test } from "@playwright/test";
import { openFixture } from "./helpers.ts";

async function choose(page: Page, mode: "Light" | "Dark" | "System") {
  await page.getByRole("button", { name: "Appearance" }).click();
  await page.getByRole("menuitemradio", { name: mode }).click();
}

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
  test("the appearance choice changes the colours (through the view transition) and survives a reload", async ({
    page,
  }) => {
    await page.goto("/studio/playground");
    await choose(page, "Light");
    const light = await token(page, "--background");
    await choose(page, "Dark");
    await expect(page.locator("html")).toHaveClass(/\bdark\b/);
    const dark = await token(page, "--background");
    expect(dark).not.toBe(light);
    await page.reload();
    await expect(page.locator("html")).toHaveClass(/\bdark\b/);
    expect(await token(page, "--background")).toBe(dark);
    // "System" follows the operating system's scheme.
    await choose(page, "System");
    await page.emulateMedia({ colorScheme: "light" });
    await expect(page.locator("html")).not.toHaveClass(/\bdark\b/);
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
  // Page titles are set in DM Sans, the public site's display face (D-182), self-hosted like
  // Inter and actually loaded; everything else people read stays Inter.
  expect(
    await page.evaluate(
      () => getComputedStyle(document.querySelector("h1") ?? document.body).fontFamily,
    ),
  ).toMatch(/^"?DM Sans Variable"?,/);
  expect(
    await page.evaluate(async () => {
      await document.fonts.ready;
      return document.fonts.check('560 32px "DM Sans Variable"');
    }),
  ).toBe(true);
});

// The design system's own page (D-182): the tokens, the one status badge and every component.

for (const scheme of ["light", "dark"] as const) {
  for (const contrast of ["no-preference", "more"] as const) {
    test(`the design system page has no accessibility violations: ${scheme}, ${contrast === "more" ? "more" : "standard"} contrast`, async ({
      page,
    }) => {
      await page.emulateMedia({ colorScheme: scheme, contrast });
      await page.goto("/studio/design");
      await expect(page.locator("html")).toHaveClass(
        scheme === "dark" ? /\bdark\b/ : /^(?!.*\bdark\b)/,
      );
      await expect(page.getByTestId("status-live")).toBeVisible();
      expect(await axe(page), "design").toEqual([]);
    });
  }
}

test("every status badge has an icon and a word, and its tone's ground", async ({
  page,
}) => {
  await page.goto("/studio/design");
  const badges = page.locator('#status [data-slot="status-badge"]');
  await expect(badges.first()).toBeVisible();
  expect(await badges.count()).toBeGreaterThanOrEqual(20);
  for (const badge of await badges.all()) {
    await expect(badge.locator("svg")).toHaveCount(1);
    expect((await badge.textContent())?.trim()).not.toBe("");
    const tone = await badge.getAttribute("data-tone");
    const [ground, expected] = await badge.evaluate((element, name) => {
      const probe = document.createElement("span");
      probe.style.background = `var(--${name}-soft)`;
      document.body.append(probe);
      const colour = getComputedStyle(probe).backgroundColor;
      probe.remove();
      return [getComputedStyle(element).backgroundColor, colour];
    }, tone);
    expect(ground, `${await badge.getAttribute("data-state")}`).toBe(expected);
  }
  // The six states an editor meets most, in the words the brief fixed.
  for (const [state, word] of [
    ["live", "Live"],
    ["changed", "Live with changes"],
    ["draft", "Draft"],
    ["scheduled", "Scheduled"],
    ["review", "In review"],
    ["taken_down", "Taken down"],
  ] as const) {
    await expect(page.getByTestId(`status-${state}`)).toHaveText(word);
  }
});

test("buttons are flat, with one accent primary and quiet secondaries", async ({
  page,
}) => {
  await page.goto("/studio/design");
  const look = (name: string) =>
    page
      .locator("#actions")
      .getByRole("button", { name, exact: true })
      .first()
      .evaluate((element) => {
        const style = getComputedStyle(element);
        return { ground: style.backgroundColor, shadow: style.boxShadow, radius: style.borderRadius };
      });
  const primary = await look("Publish");
  const secondary = await look("Preview");
  const accent = await page.evaluate(() => {
    const probe = document.createElement("span");
    probe.style.background = "var(--primary)";
    document.body.append(probe);
    const colour = getComputedStyle(probe).backgroundColor;
    probe.remove();
    return colour;
  });
  expect(primary.ground).toBe(accent);
  expect(secondary.ground).not.toBe(accent);
  for (const button of [primary, secondary]) {
    expect(button.shadow).toBe("none");
    expect(button.radius).toBe("8px");
  }
});

test("a dialog keeps to a phone's screen: its body scrolls, its footer stays, its fields stack", async ({
  page,
}) => {
  await page.setViewportSize({ width: 390, height: 640 });
  await page.goto("/studio/design");
  await page.getByTestId("long-dialog").click();
  const dialog = page.getByTestId("long-dialog-content");
  await expect(dialog).toBeVisible();
  const box = await dialog.boundingBox();
  expect(box).not.toBeNull();
  expect((box?.y ?? 0) + (box?.height ?? 0)).toBeLessThanOrEqual(640);
  expect(
    await dialog.evaluate((element) => element.scrollHeight > element.clientHeight),
    "the body scrolls",
  ).toBe(true);
  await expect(dialog.getByRole("button", { name: "Create task" })).toBeInViewport();
  // The footer reaches the dialog's bottom edge: no strip of the body shows beneath it.
  const footer = await dialog.locator('[data-slot="dialog-footer"]').boundingBox();
  expect(
    Math.abs((footer?.y ?? 0) + (footer?.height ?? 0) - ((box?.y ?? 0) + (box?.height ?? 0))),
  ).toBeLessThanOrEqual(1);
  const label = await dialog.getByText("Who is it for?", { exact: true }).boundingBox();
  const field = await dialog.locator("#l-who").boundingBox();
  expect((label?.y ?? 0) + (label?.height ?? 0), "the label sits above its field").toBeLessThanOrEqual(field?.y ?? 0);
  expect(field?.width ?? 0, "the field takes the dialog's width").toBeGreaterThan(280);
});

test("checkboxes and radios take the accent when chosen, native ones too, in both themes", async ({
  page,
}) => {
  for (const scheme of ["light", "dark"] as const) {
    await page.emulateMedia({ colorScheme: scheme });
    await page.goto("/studio/design");
    await expect(page.locator("html")).toHaveClass(
      scheme === "dark" ? /\bdark\b/ : /^(?!.*\bdark\b)/,
    );
    await expect(page.getByTestId("status-live")).toBeVisible();
    const accent = await page.evaluate(() => {
      const probe = document.createElement("span");
      probe.style.background = "var(--primary)";
      document.body.append(probe);
      const colour = getComputedStyle(probe).backgroundColor;
      probe.remove();
      return colour;
    });
    const ground = (selector: string) =>
      page.locator(selector).first().evaluate((element) => getComputedStyle(element).backgroundColor);
    expect(await ground('#forms button[role="checkbox"][data-state="checked"]'), scheme).toBe(accent);
    expect(await ground('#forms input[type="checkbox"]:checked'), scheme).toBe(accent);
    expect(await ground('#forms input[type="radio"]:checked'), scheme).toBe(accent);
    expect(await ground('#forms input[type="checkbox"]:not(:checked)'), scheme).not.toBe(accent);
  }
});
