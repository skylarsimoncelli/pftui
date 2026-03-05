import fs from "node:fs";
import path from "node:path";
import { expect, test, type Page } from "@playwright/test";
import { installWebMocks } from "./web.mocks";

const THEMES = [
  "midnight",
  "catppuccin",
  "nord",
  "dracula",
  "solarized",
  "gruvbox",
  "inferno",
  "neon",
  "hacker",
  "pastel",
  "miasma",
];

async function captureThemeSet(page: Page, viewportName: string) {
  const outDir = path.join("artifacts", "visual", viewportName);
  fs.mkdirSync(outDir, { recursive: true });

  for (const [idx, theme] of THEMES.entries()) {
    if (idx > 0) {
      await page.getByRole("button", { name: /^Theme:/ }).click();
      await page.waitForTimeout(80);
    }
    const file = path.join(outDir, `${theme}.png`);
    await page.screenshot({ path: file, fullPage: true });
    expect(fs.existsSync(file)).toBeTruthy();
  }
}

test("visual snapshots desktop + mobile across themes", async ({ page }) => {
  await installWebMocks(page);
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto("/");
  await captureThemeSet(page, "desktop");

  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto("/");
  await captureThemeSet(page, "mobile");
});
