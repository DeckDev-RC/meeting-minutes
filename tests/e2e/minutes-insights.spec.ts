import { expect, test, type Page } from "@playwright/test";

const meetingId = "minutes-insights-e2e";

test.describe.configure({ timeout: 60_000 });

async function installMinutesMock(page: Page) {
  await page.addInitScript((id) => {
    window.__MEETING_MINUTES_E2E__ = {
      listen: async () => () => undefined,
      invoke: async (command, args = {}) => {
        if (command === "get_minutes_by_meeting") {
          if (args.meetingId !== id) return null;
          return {
            id: "minutes-1",
            meeting_id: id,
            html_content:
              '<section><h1>Ata de Reuniao</h1><p>Resumo final da reuniao.</p></section>',
            pdf_path: null,
            model_used: "gemini-2.5-flash",
            created_at: "2026-05-22T12:00:00Z",
          };
        }

        if (command === "get_processing_chunks") {
          return [
            {
              meetingId: id,
              index: 0,
              audioPath: "chunk-000.flac",
              startSec: 0,
              endSec: 120,
              offsetSec: 0,
              durationSec: 120,
              status: "done",
              rawSegmentsJson: "[]",
              errorMsg: null,
              factsStatus: "done",
              factsJson: JSON.stringify({
                chunkIndex: 0,
                startSec: 0,
                endSec: 120,
                summary: "Equipe alinhou correcao do leitor de documentos.",
                topics: ["Leitor de documentos", "Drive"],
                decisions: [
                  {
                    title: "Manter Caio como responsavel pela correcao.",
                    owner: "Caio",
                    timestampSec: 42,
                    evidence: "Caio fica responsavel pela correcao",
                  },
                ],
                actions: [
                  {
                    task: "Revisar arquivos sincronizados no Drive",
                    owner: "Emanuella",
                    deadline: "sexta-feira",
                    timestampSec: 78,
                    evidence: "revisar arquivos do Drive",
                  },
                ],
                questions: ["Como validar PDFs que vieram do WhatsApp?"],
                risks: ["Relatorio pode falhar com Drive desatualizado"],
              }),
              factsErrorMsg: null,
            },
          ];
        }

        throw new Error(`Unhandled mock command: ${command}`);
      },
    };
  }, meetingId);
}

test.beforeEach(async ({ page }) => {
  await installMinutesMock(page);
});

test("minutes page keeps generated insights available after the final minutes", async ({
  page,
}, testInfo) => {
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

  await page.goto(`/minutes/${meetingId}`);

  await expect(page.getByRole("heading", { name: "Ata da reuniao" })).toBeVisible({
    timeout: 45_000,
  });
  await expect(page.getByText("Resumo final da reuniao.")).toBeVisible();

  await page.getByRole("button", { name: /Insights\s*1/ }).click();

  await expect(page.getByRole("heading", { name: "Insights extraidos" })).toBeVisible();
  await expect(page.getByText("Equipe alinhou correcao do leitor")).toBeVisible();
  await expect(page.getByText("Manter Caio como responsavel")).toBeVisible();
  await expect(page.getByText("Revisar arquivos sincronizados no Drive")).toBeVisible();
  await expect(page.getByText("Relatorio pode falhar")).toBeVisible();

  await page.screenshot({
    path: testInfo.outputPath("minutes-insights.png"),
    fullPage: false,
  });

  expect(consoleMessages).toEqual([]);
});
