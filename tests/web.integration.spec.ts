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
    await expect(page.locator("#view-positions")).not.toContainText("Market Overview");

    await page.getByRole("button", { name: "Markets" }).click();
    await expect(page.locator("#view-markets")).toHaveClass(/active/);
    await expect(page.locator("#view-markets")).toContainText("Market Overview");
    await expect(page.locator("#market-breadth")).toContainText("Up / Down / Flat");
    await expect(page.locator("#market-movers")).toContainText("S&P 500");

    await page.getByRole("button", { name: "Economy" }).click();
    await expect(page.locator("#view-economy")).toHaveClass(/active/);
    await expect(page.locator("#economy-snapshot")).toContainText("BLS Pulse");
    await expect(page.locator("#economy-snapshot")).toContainText("Non-Farm Payrolls");

    await page.getByRole("button", { name: "Watchlist" }).click();
    await expect(page.locator("#view-watchlist")).toHaveClass(/active/);

    await page.getByRole("button", { name: "Positions" }).click();
    await expect(page.locator("#view-positions")).toHaveClass(/active/);

    await page.locator("#positions-table tbody tr").first().click();
    await expect(page.locator("#asset-detail-drawer")).toHaveClass(/active/);
    await expect(page.locator("#tradingview-chart")).toBeVisible();
  });

  test("search overlay uses global API and supports star toggle", async ({ page }) => {
    await page.keyboard.press("/");
    await expect(page.locator("#search-overlay")).toHaveClass(/active/);

    await page.locator("#global-search-input").fill("MSFT");
    await expect(page.locator("#global-search-results .search-row").first()).toContainText("MSFT");
    await page.keyboard.press("Enter");

    await expect(page.locator("#asset-detail-drawer")).toHaveClass(/active/);
    await expect(page.locator("#asset-detail-body")).toContainText("1W Change");
    await page.getByRole("button", { name: "Star Watchlist" }).click();
    await expect(page.getByRole("button", { name: "Unstar Watchlist" })).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(page.locator("#asset-detail-drawer")).not.toHaveClass(/active/);
  });

  test("alerts create, acknowledge, rearm, and remove", async ({ page }) => {
    await page.locator("#tabs").getByRole("button", { name: /Alerts/ }).click();
    await expect(page.locator("#view-alerts")).toHaveClass(/active/);

    await page.locator("#alert-rule-input").fill("MSFT below 400");
    await page.getByRole("button", { name: "Create Alert" }).click();
    await expect(page.locator("#alerts-list")).toContainText("MSFT below 400");

    await page.getByRole("button", { name: "Ack" }).first().click();
    await expect(page.locator("#alerts-list")).toContainText("acknowledged");

    await page.getByRole("button", { name: "Rearm" }).first().click();
    await expect(page.locator("#alerts-list")).toContainText("armed");

    const before = await page.locator("#alerts-list .list-item").count();
    await page.getByRole("button", { name: "Remove" }).first().click();
    await expect(page.locator("#alerts-list .list-item")).toHaveCount(before - 1);
  });

  test("journal create, edit, and delete", async ({ page }) => {
    await page.locator("#tabs").getByRole("button", { name: "Journal" }).click();
    await expect(page.locator("#view-journal")).toHaveClass(/active/);

    await page.locator("#journal-create-input").fill("New macro thesis entry");
    await page.locator("#journal-symbol-input").fill("MSFT");
    await page.locator("#journal-tag-input").fill("thesis");
    await page.getByRole("button", { name: "Add Entry" }).click();
    await expect(page.locator("#journal-list")).toContainText("New macro thesis entry");

    await page.locator("#journal-list .list-item").first().click();
    await page.locator("#journal-edit-content").fill("Updated macro thesis entry");
    await page.locator("#journal-edit-status").selectOption("validated");
    await page.getByRole("button", { name: "Save" }).click();
    await expect(page.locator("#journal-list")).toContainText("validated");
    await expect(page.locator("#journal-detail")).toContainText("Updated macro thesis entry");

    const before = await page.locator("#journal-list .list-item").count();
    await page.getByRole("button", { name: "Delete" }).click();
    await expect(page.locator("#journal-list .list-item")).toHaveCount(before - 1);
  });

  test("news timeline renders chronologically and filters", async ({ page }) => {
    await page.locator("#tabs").getByRole("button", { name: "News" }).click();
    await expect(page.locator("#view-news")).toHaveClass(/active/);
    await expect(page.locator("#view-news")).toContainText("News Timeline");
    await expect(page.locator("#news-list .timeline-heading")).toHaveCount(2);
    await expect(page.locator("#news-list")).toContainText("CoinDesk");

    await page.locator("#news-category-filter").selectOption("macro");
    await expect(page.locator("#news-list")).toContainText("Fed officials signal caution on early cuts");
    await expect(page.locator("#news-list")).not.toContainText("Bitcoin options skew flips toward calls");

    await page.locator("#news-source-filter").fill("Bloom");
    await expect(page.locator("#news-list")).toContainText("Bloomberg");
  });

  test("transactions create, edit, and delete", async ({ page }) => {
    await page.locator("#tabs").getByRole("button", { name: "Transactions" }).click();
    await expect(page.locator("#view-transactions")).toHaveClass(/active/);

    await page.locator("#tx-form-symbol").fill("MSFT");
    await page.locator("#tx-form-category").selectOption("equity");
    await page.locator("#tx-form-type").selectOption("buy");
    await page.locator("#tx-form-qty").fill("3");
    await page.locator("#tx-form-price").fill("400");
    await page.locator("#tx-form-date").fill("2026-03-06");
    await page.getByRole("button", { name: "Add" }).click();
    await expect(page.locator("#transactions-table")).toContainText("MSFT");

    await page.getByRole("button", { name: "Edit" }).first().click();
    await page.locator("#tx-form-price").fill("405");
    await page.getByRole("button", { name: "Update" }).click();
    await expect(page.locator("#transactions-table")).toContainText("$405.00");

    const before = await page.locator("#transactions-table tbody tr").count();
    await page.getByRole("button", { name: "Delete" }).first().click();
    await expect(page.locator("#transactions-table tbody tr")).toHaveCount(before - 1);
  });
});
