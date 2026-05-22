import { expect, test, type Page } from "@playwright/test";

async function installSettingsMock(page: Page) {
  await page.addInitScript(() => {
    const savedPayloads: Array<Record<string, unknown> | undefined> = [];

    window.__MEETING_MINUTES_E2E__ = {
      listen: async () => () => undefined,
      invoke: async (command, args = {}) => {
        if (command === "get_api_keys") {
          return {
            groq: "gsk_saved",
            gemini: "gemini_saved",
            cloudflareAccountId: "",
            cloudflareApiToken: "",
            deepgramApiKey: "",
            transcriptionProfile: "smart-low-cost",
            manualTranscriptionProvider: "groq",
            expectedSpeakers: 3,
          };
        }

        if (command === "set_api_keys") {
          savedPayloads.push(args);
          (window as unknown as { __SAVED_SETTINGS__: unknown }).__SAVED_SETTINGS__ = args;
          return undefined;
        }

        throw new Error(`Unhandled mock command: ${command}`);
      },
    };
  });
}

test.beforeEach(async ({ page }) => {
  await installSettingsMock(page);
});

test("settings saves adaptive transcription providers and profile", async ({ page }) => {
  const consoleMessages: string[] = [];
  page.on("console", (message) => {
    if (
      message.type() === "warning" &&
      message.text().includes("React Router Future Flag Warning")
    ) {
      return;
    }
    if (["error", "warning"].includes(message.type())) {
      consoleMessages.push(`${message.type()}: ${message.text()}`);
    }
  });

  await page.goto("/settings");

  await expect(page.getByRole("heading", { name: "Configuracoes" })).toBeVisible({
    timeout: 20_000,
  });
  await expect(page.getByText("Transcricao adaptativa")).toBeVisible();

  await page.getByLabel("Perfil de transcricao").selectOption("smart-low-cost");
  await page.getByLabel("Chave API Groq").fill("gsk_live");
  await page.getByLabel("Cloudflare Account ID").fill("cf-account");
  await page.getByLabel("Cloudflare API Token").fill("cfat_token");
  await page.getByLabel("Chave API Deepgram").fill("deepgram-token");
  await page.getByLabel("Chave API Gemini").fill("gemini-token");
  await page.getByLabel("Numero esperado de falantes").selectOption("4");
  await page.getByRole("button", { name: "Salvar configuracoes" }).click();

  await expect(page.getByText("Chaves salvas com sucesso.")).toBeVisible();

  const payload = await page.evaluate(
    () => (window as unknown as { __SAVED_SETTINGS__: Record<string, unknown> }).__SAVED_SETTINGS__,
  );

  expect(payload).toMatchObject({
    groq: "gsk_live",
    gemini: "gemini-token",
    cloudflareAccountId: "cf-account",
    cloudflareApiToken: "cfat_token",
    deepgramApiKey: "deepgram-token",
    transcriptionProfile: "smart-low-cost",
    manualTranscriptionProvider: "groq",
    expectedSpeakers: 4,
  });
  expect(consoleMessages).toEqual([]);
});
