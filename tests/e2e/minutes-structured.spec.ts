import { expect, test, type Page } from "@playwright/test";

const structuredMeetingId = "minutes-structured-e2e";
const legacyMeetingId = "minutes-legacy-e2e";

test.describe.configure({ timeout: 60_000 });

async function installStructuredMinutesMock(page: Page) {
  await page.addInitScript(({ structuredId, legacyId }) => {
    const savedPdfs: Array<{ suggestedName: string; size: number }> = [];
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
            userEdited: false,
            participantNames: ["Caio", "Rafaela"],
            purgeSummary: {
              removedTopics: 1,
              removedDecisions: 1,
              removedActions: 1,
              removedTotal: 3,
            },
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

        if (command === "save_pdf") {
          const bytes = Array.isArray(args.pdfBytes) ? args.pdfBytes : [];
          savedPdfs.push({
            suggestedName: String(args.suggestedName ?? ""),
            size: bytes.length,
          });
          (window as typeof window & { __SAVED_PDFS__?: typeof savedPdfs }).__SAVED_PDFS__ =
            savedPdfs;
          return `C:\\exports\\${String(args.suggestedName ?? "ata.pdf")}`;
        }

        if (command === "save_html") {
          return `C:\\exports\\${String(args.suggestedName ?? "ata.html")}`;
        }

        if (command === "open_folder") {
          return undefined;
        }

        throw new Error(`Unhandled mock command: ${command}`);
      },
    };
    (window as typeof window & { __SAVED_PDFS__?: typeof savedPdfs }).__SAVED_PDFS__ =
      savedPdfs;
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
  await expect(page.getByRole("button", { name: "PDF executivo" })).toBeVisible();
  await page.getByLabel("Abrir opcoes de exportacao").click();
  await expect(page.getByRole("button", { name: "PDF completo" })).toBeVisible();
  await expect(page.getByRole("button", { name: "HTML completo" })).toBeVisible();
  await expect(page.getByRole("heading", { name: "Fila de revisao" })).toBeVisible();
  await expect(page.getByText("Evidencias fracas").first()).toBeVisible();
  await expect(page.getByText("Resumo final estruturado.")).toBeVisible();
  await expect(page.getByText("1 evidencia precisa de revisao")).toBeVisible();
  await expect(page.getByText("Purge anti-alucinacao removeu 3 itens sem evidencia")).toBeVisible();
  await expect(page.getByText("1 topico, 1 decisao, 1 acao")).toBeVisible();

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

test("executive PDF export renders content and dark menu keeps readable colors", async ({ page }) => {
  await page.addInitScript(() => {
    window.localStorage.setItem("meeting-minutes-theme", "dark");
  });
  await page.goto(`/minutes/${structuredMeetingId}`);

  await expect(page.getByRole("heading", { name: "Ata da reuniao" })).toBeVisible({
    timeout: 45_000,
  });
  await page.getByLabel("Abrir opcoes de exportacao").click();
  const executiveMenuItem = page.getByRole("button", { name: "PDF executivo" }).nth(1);
  await expect(executiveMenuItem).toBeVisible();
  const menuColors = await executiveMenuItem.evaluate((element) => {
    const style = window.getComputedStyle(element);
    const parentStyle = window.getComputedStyle(element.parentElement as Element);
    return {
      color: style.color,
      backgroundColor: parentStyle.backgroundColor,
    };
  });
  expect(menuColors.color).not.toBe(menuColors.backgroundColor);

  await executiveMenuItem.click();
  await expect
    .poll(
      () =>
        page.evaluate(() => {
          const saved =
            (window as typeof window & {
              __SAVED_PDFS__?: Array<{ suggestedName: string; size: number }>;
            }).__SAVED_PDFS__ ?? [];
          return saved.at(-1) ?? null;
        }),
      { timeout: 30_000 },
    )
    .toEqual(
      expect.objectContaining({
        suggestedName: expect.stringContaining("executiva.pdf"),
        size: expect.any(Number),
      }),
    );
  const pdfSize = await page.evaluate(() => {
    const saved =
      (window as typeof window & {
        __SAVED_PDFS__?: Array<{ suggestedName: string; size: number }>;
      }).__SAVED_PDFS__ ?? [];
    return saved.at(-1)?.size ?? 0;
  });
  expect(pdfSize).toBeGreaterThan(10_000);
});
