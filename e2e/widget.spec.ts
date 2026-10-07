import { expect, test } from "@playwright/test";

test("widget chat shows a fake AI reply on one conversation", async ({
  request,
  page,
}) => {
  await expect
    .poll(async () => {
      const health = await request.get("/healthz");
      if (!health.ok()) return null;
      const body = await health.json();
      return body.uctp ?? null;
    })
    .toBeTruthy();

  const tokenRes = await request.post("/v1/widget/tokens", {
    headers: {
      authorization: "Bearer dev-only",
      "content-type": "application/json",
    },
    data: { visitor_id: "usr_playwright", origin: "http://127.0.0.1:18080" },
  });
  expect(tokenRes.ok()).toBeTruthy();
  const { token } = await tokenRes.json();
  expect(token).toBeTruthy();

  await page.goto(`/widget/?token=${encodeURIComponent(token)}`);
  await expect(page.getByRole("button", { name: "Message" })).toBeVisible();
  await page.locator("#draft").fill("what are your hours?");
  await page.getByRole("button", { name: "Message" }).click();
  await expect(page.locator("#thread")).toContainText("what are your hours?", {
    timeout: 30_000,
  });
  await expect(page.locator("#thread")).toContainText("fake-reply:", {
    timeout: 30_000,
  });
});

test("desk login lists a conversation", async ({ request, page }) => {
  await expect
    .poll(async () => {
      const health = await request.get("/healthz");
      return health.ok();
    })
    .toBeTruthy();

  const created = await request.post("/v1/conversations", {
    headers: {
      authorization: "Bearer dev-only",
      "content-type": "application/json",
    },
    data: { identity: { visitor_id: "usr_desk_e2e" } },
  });
  expect(created.ok()).toBeTruthy();
  const body = await created.json();
  expect(body.id).toBeTruthy();

  await page.goto("/desk/");
  await page.locator("#bootstrap").fill("bootstrap");
  await page.getByRole("button", { name: "Login" }).click();
  await expect(page.locator("#inbox")).toContainText("usr_desk_e2e", {
    timeout: 15_000,
  });
});

test("desk inbox updates over SSE without refresh", async ({ request, page }) => {
  await expect
    .poll(async () => {
      const health = await request.get("/healthz");
      return health.ok();
    })
    .toBeTruthy();

  await page.goto("/desk/");
  await page.locator("#bootstrap").fill("bootstrap");
  await page.getByRole("button", { name: "Login" }).click();
  await expect(page.locator("#inbox")).toBeVisible();
  await page.waitForTimeout(400);

  const visitor = `usr_desk_live_${Date.now()}`;
  const created = await request.post("/v1/conversations", {
    headers: {
      authorization: "Bearer dev-only",
      "content-type": "application/json",
    },
    data: { identity: { visitor_id: visitor } },
  });
  expect(created.ok()).toBeTruthy();
  await expect(page.locator("#inbox")).toContainText(visitor, {
    timeout: 15_000,
  });
});
