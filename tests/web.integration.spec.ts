import { expect, test } from "@playwright/test";
import { installWebMocks } from "./web.mocks";

test.describe("web integration flows", () => {
  test.beforeEach(async ({ page }) => {
    await installWebMocks(page);
    await page.goto("/");
  });

  test("tab navigation and chart/detail flow", async ({ page }) => {
    await expect(page.locator("#tabs .tab-btn")).toHaveCount(8);
    await expect(page.locator("#view-positions")).toHaveClass(/active/);

    await page.getByRole("button", { name: "Markets" }).click();
    await expect(page.locator("#view-markets")).toHaveClass(/active/);

    await page.getByRole("button", { name: "Watchlist" }).click();
    await expect(page.locator("#view-watchlist")).toHaveClass(/active/);

    await page.getByRole("button", { name: "Positions" }).click();
    await expect(page.locator("#view-positions")).toHaveClass(/active/);

    await page.locator("#positions-table tbody tr").first().click();
    await expect(page.locator("#asset-detail-drawer")).toHaveClass(/active/);
    await expect(page.locator("#tradingview-chart")).toBeVisible();
  });

  test("search overlay keyboard routing", async ({ page }) => {
    await page.keyboard.press("/");
    await expect(page.locator("#search-overlay")).toHaveClass(/active/);

    await page.locator("#global-search-input").fill("AAPL");
    await page.keyboard.press("Enter");

    await expect(page.locator("#asset-detail-drawer")).toHaveClass(/active/);
    await page.keyboard.press("Escape");
    await expect(page.locator("#asset-detail-drawer")).not.toHaveClass(/active/);
  });
});
