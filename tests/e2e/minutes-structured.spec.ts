import { expect, test, type Page } from "@playwright/test";

const structuredMeetingId = "minutes-structured-e2e";
const legacyMeetingId = "minutes-legacy-e2e";

test.describe.configure({ timeout: 60_000 });

async function installStructuredMinutesMock(page: Page) {
  await page.addInitScript(({ structuredId, legacyId }) => {
    window.__MEETING_MINUTES_E2E__ = {
      listen: async () => () => undefined,
      invoke: async (command, args = {}) => {
        if (command === "get_structured_minutes_by_meeting") {
          if (args.meetingId === legacyId) return null;
          if (args.meetingId !== structuredId) return null;
          return {
            minuteId: "minute-structured-1",
            meetingId: structuredId,
            htmlContent:
              "<section><h1>Ata estruturada</h1><p>Resumo final estruturado.</p></section>",
            pdfPath: null,
            modelUsed: "gemini-2.5-flash",
            createdAt: "2026-05-23T11:00:00Z",
            decisions: [
              {
                id: "decision-1",
                minuteId: "minute-structured-1",
                meetingId: structuredId,
                itemIndex: 0,
                chunkIndex: 0,
                title: "Aprovar entrega da ata estruturada",
                owner: "Caio",
                timestampSec: 42,
                evidence: "Caio aprovou a entrega da ata estruturada",
                evidenceId: "evidence-1",
                createdAt: "2026-05-23T11:00:00Z",
              },
            ],
            actions: [
              {
                id: "action-1",
                minuteId: "minute-structured-1",
                meetingId: structuredId,
                itemIndex: 0,
                chunkIndex: 1,
                task: "Revisar evidencias fracas",
                owner: "Rafaela",
                deadline: "sexta-feira",
                timestampSec: 84,
                evidence: "Rafaela revisa as evidencias fracas",
                evidenceId: "evidence-2",
                createdAt: "2026-05-23T11:01:00Z",
              },
            ],
            evidences: [
              {
                id: "evidence-1",
                minuteId: "minute-structured-1",
                meetingId: structuredId,
                parentType: "decision",
                parentId: "decision-1",
                chunkIndex: 0,
                quote: "Caio aprovou a entrega da ata estruturada",
                transcriptExcerpt: "Caio aprovou a entrega da ata estruturada no fim da reuniao.",
                validated: true,
                validationScore: 0.96,
                createdAt: "2026-05-23T11:00:00Z",
              },
              {
                id: "evidence-2",
                minuteId: "minute-structured-1",
                meetingId: structuredId,
                parentType: "action",
                parentId: "action-1",
                chunkIndex: 1,
                quote: "Rafaela revisa as evidencias fracas",
                transcriptExcerpt: null,
                validated: false,
                validationScore: 0.31,
                createdAt: "2026-05-23T11:01:00Z",
              },
            ],
            versions: [
              {
                id: "version-1",
                minuteId: "minute-structured-1",
                meetingId: structuredId,
                versionNo: 1,
                createdAt: "2026-05-23T11:00:00Z",
              },
            ],
          };
        }

        if (command === "get_minute_evidences") {
          return [];
        }

        if (command === "get_minutes_by_meeting") {
          if (args.meetingId === legacyId) {
            return {
              id: "minute-legacy-1",
              meeting_id: legacyId,
              html_content:
                "<section><h1>Ata legada</h1><p>Resumo vindo do HTML antigo.</p></section>",
              pdf_path: null,
              model_used: "gemini-2.5-flash",
              created_at: "2026-05-22T12:00:00Z",
            };
          }
          return null;
        }

        if (command === "get_processing_chunks") {
          if (args.meetingId !== legacyId) return [];
          return [
            {
              meetingId: legacyId,
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
                summary: "Resumo legado por chunk.",
                topics: ["Legado"],
                decisions: [],
                actions: [],
                questions: [],
                risks: [],
              }),
              factsErrorMsg: null,
            },
          ];
        }

        if (command === "get_transcription_by_meeting") {
          return null;
        }

        throw new Error(`Unhandled mock command: ${command}`);
      },
    };
  }, { structuredId: structuredMeetingId, legacyId: legacyMeetingId });
}

test.beforeEach(async ({ page }) => {
  await installStructuredMinutesMock(page);
});

test("minutes page renders structured decisions actions and evidence tabs", async ({ page }) => {
  await page.goto(`/minutes/${structuredMeetingId}`);

  await expect(page.getByRole("heading", { name: "Ata da reuniao" })).toBeVisible({
    timeout: 45_000,
  });
  await expect(page.getByRole("heading", { name: "Central de revisao" })).toBeVisible();
  await expect(page.getByRole("button", { name: "Revisar evidencias" })).toBeVisible();
  await expect(page.getByRole("button", { name: "Mapear falantes" })).toBeVisible();
  await expect(page.getByRole("heading", { name: "Fila de revisao" })).toBeVisible();
  await expect(page.getByText("Evidencias fracas").first()).toBeVisible();
  await expect(page.getByText("Resumo final estruturado.")).toBeVisible();
  await expect(page.getByText("1 evidencia precisa de revisao")).toBeVisible();

  await page.getByRole("button", { name: /Decisoes\s*1/ }).click();
  await expect(page.getByRole("heading", { name: "Decisoes estruturadas" })).toBeVisible();
  await expect(page.getByText("Aprovar entrega da ata estruturada").first()).toBeVisible();
  await expect(page.getByText("Responsavel: Caio")).toBeVisible();

  await page.getByRole("button", { name: /Acoes\s*1/ }).click();
  await expect(page.getByRole("heading", { name: "Acoes estruturadas" })).toBeVisible();
  await expect(page.getByText("Revisar evidencias fracas").first()).toBeVisible();
  await expect(page.getByText("Prazo: sexta-feira")).toBeVisible();

  await page.getByRole("button", { name: /Evidencias\s*2/ }).click();
  await expect(page.getByRole("heading", { name: "Evidencias da ata" })).toBeVisible();
  await expect(page.getByText("Verificada - 96%")).toBeVisible();
  await expect(page.getByText("Fraca - 31%")).toBeVisible();
  await expect(page.getByText("Rafaela revisa as evidencias fracas").first()).toBeVisible();
});

test("minutes page falls back to legacy html when structured data is absent", async ({ page }) => {
  await page.goto(`/minutes/${legacyMeetingId}`);

  await expect(page.getByRole("heading", { name: "Ata da reuniao" })).toBeVisible({
    timeout: 45_000,
  });
  await expect(page.getByText("Resumo vindo do HTML antigo.")).toBeVisible();
  await expect(page.getByText("Ata antiga sem estrutura persistida")).toBeVisible();
  await page.getByRole("button", { name: /Insights\s*1/ }).click();
  await expect(page.getByText("Resumo legado por chunk.")).toBeVisible();
});
