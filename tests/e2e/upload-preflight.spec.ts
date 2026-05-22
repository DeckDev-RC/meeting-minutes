import { expect, test, type Page } from "@playwright/test";

async function installUploadMock(page: Page) {
  await page.addInitScript(() => {
    window.__MEETING_MINUTES_E2E__ = {
      listen: async () => () => undefined,
      invoke: async (command, args = {}) => {
        if (command === "get_api_keys") {
          return {
            groq: "groq-key",
            gemini: "gemini-key",
            cloudflareAccountId: "cloudflare-account",
            cloudflareApiToken: "cloudflare-token",
            deepgramApiKey: "deepgram-key",
            transcriptionProfile: "smart-low-cost",
            manualTranscriptionProvider: "groq",
            expectedSpeakers: 4,
          };
        }

        if (command === "probe_media_metadata") {
          return {
            sourcePath: args.inputPath,
            sourceFileName: "meeting.mp4",
            recordedAt: "2026-05-22T12:00:00Z",
            durationSec: 10_800,
          };
        }

        if (command === "save_meeting") {
          (window as unknown as { __SAVED_MEETING__: unknown }).__SAVED_MEETING__ = args.meeting;
          return "meeting-upload-e2e";
        }

        throw new Error(`Unhandled mock command: ${command}`);
      },
    };
  });
}

async function dropAcceptedFile(page: Page) {
  await page.locator("text=Arraste seu audio ou video").evaluate((target) => {
    const file = new File(["audio"], "meeting.mp4", { type: "video/mp4" });
    const dataTransfer = new DataTransfer();
    dataTransfer.items.add(file);
    target.closest("div")?.dispatchEvent(
      new DragEvent("drop", {
        bubbles: true,
        cancelable: true,
        dataTransfer,
      }),
    );
  });
}

test.beforeEach(async ({ page }) => {
  await installUploadMock(page);
});

test("upload pre-check shows route, quota risk and saves meeting transcription profile", async ({
  page,
}) => {
  await page.goto("/upload");

  await expect(page.getByRole("heading", { name: "Nova reuniao" })).toBeVisible({
    timeout: 20_000,
  });

  await dropAcceptedFile(page);

  await expect(page.getByText("Pre-check da reuniao")).toBeVisible();
  await expect(page.getByText("3h 00min")).toBeVisible();
  await expect(page.getByText("Cloudflare Whisper")).toBeVisible();
  await expect(page.getByText("Deepgram Nova-3 -> Groq Whisper -> faster-whisper local")).toBeVisible();
  await expect(page.getByText("US$ 1.28 se usado")).toBeVisible();

  await page.getByRole("radio", { name: /R\$ 0 offline/ }).click();
  await expect(page.getByText("Parakeet local")).toBeVisible();
  await expect(page.getByText("US$ 0.00 se usado")).toBeVisible();

  await page.getByRole("button", { name: "Processar reuniao" }).click();

  const payload = await page.evaluate(
    () => (window as unknown as { __SAVED_MEETING__: Record<string, unknown> }).__SAVED_MEETING__,
  );

  expect(payload).toMatchObject({
    filePath: "meeting.mp4",
    processingProfile: "balanced",
    transcriptionProfile: "offline-free",
    status: "processing",
  });
});
