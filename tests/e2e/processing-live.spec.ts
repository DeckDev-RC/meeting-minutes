import { expect, test, type Page } from "@playwright/test";

const meetingId = "e2e-ui-meeting";

test.describe.configure({ timeout: 90_000 });

async function installTauriMock(page: Page) {
  await page.addInitScript((id) => {
    const delay = (ms: number) => new Promise((resolve) => window.setTimeout(resolve, ms));
    const listeners = new Map<string, Set<(event: { payload: unknown }) => void>>();
    const processingJobs: Array<Record<string, unknown>> = [];
    const savedMinutes: Array<Record<string, unknown>> = [];
    const savedBenchmarkRuns: Array<Record<string, unknown>> = [];
    const chunks = [
      {
        index: 0,
        audioPath: "chunk-000.flac",
        startSec: 0,
        endSec: 120,
        offsetSec: 0,
        durationSec: 120,
      },
      {
        index: 1,
        audioPath: "chunk-001.flac",
        startSec: 120,
        endSec: 240,
        offsetSec: 120,
        durationSec: 120,
      },
    ];
    let processingChunks: Array<Record<string, unknown>> = [];

    const emit = (event: string, payload: unknown) => {
      for (const handler of listeners.get(event) || []) {
        handler({ payload });
      }
    };

    const transcriptForOffset = (offsetSec: number) => {
      if (offsetSec === 0) {
        return [
          { id: 1, start: 18, end: 22, text: "Depois o time valida os documentos." },
          { id: 0, start: 4, end: 9, text: "Primeiro precisamos ajustar o leitor de PDF." },
          { id: 2, start: 24, end: 27, text: "A planilha deve sair no formato final." },
        ];
      }

      return [
        { id: 0, start: 122, end: 126, text: "Caio fica responsavel pelo drive." },
        { id: 1, start: 128, end: 132, text: "Emanuella revisa a base de clientes." },
        { id: 2, start: 136, end: 139, text: "O prazo combinado foi sexta-feira." },
      ];
    };

    const diarizedResult = () => ({
      speakers: ["Caio", "Emanuella"],
      segments: [
        {
          speaker: "Caio",
          start: 4,
          end: 27,
          text: "Primeiro precisamos ajustar o leitor de PDF. Depois o time valida os documentos.",
        },
        {
          speaker: "Emanuella",
          start: 122,
          end: 139,
          text: "Emanuella revisa a base de clientes. O prazo combinado foi sexta-feira.",
        },
      ],
    });

    window.__MEETING_MINUTES_E2E__ = {
      listen: async (event, handler) => {
        const eventListeners = listeners.get(event) || new Set();
        eventListeners.add(handler as (event: { payload: unknown }) => void);
        listeners.set(event, eventListeners);
        return () => eventListeners.delete(handler as (event: { payload: unknown }) => void);
      },
      invoke: async (command, args = {}) => {
        await delay(15);

        if (command === "get_api_keys") {
          return {
            groq: "groq-key",
            gemini: "gemini-key",
            cloudflareAccountId: "cloudflare-account",
            cloudflareApiToken: "cloudflare-token",
            deepgramApiKey: "deepgram-key",
            transcriptionProfile: "smart-low-cost",
            manualTranscriptionProvider: "groq",
            expectedSpeakers: 2,
          };
        }
        if (command === "get_meetings") {
          return [
            {
              id,
              title: "Reuniao E2E",
              filePath: "C:\\reunioes\\e2e.mp4",
              audioPath: null,
              participantsHint: "Caio\nEmanuella",
              processingProfile: "balanced",
              transcriptionProfile: "smart-low-cost",
              status: "processing",
              createdAt: "2026-05-20T12:00:00Z",
              updatedAt: "2026-05-20T12:00:00Z",
            },
          ];
        }
        if (command === "resolve_processing_work_dir") {
          return "C:\\reunioes\\work\\e2e-ui-meeting";
        }
        if (command === "probe_media_metadata") {
          return {
            sourcePath: "C:\\reunioes\\e2e.mp4",
            sourceFileName: "e2e.mp4",
            recordedAt: "2026-05-20T12:00:00Z",
          };
        }
        if (command === "prepare_audio_and_chunks") return { durationSec: 240, chunks };
        if (command === "extract_audio") return 240;
        if (command === "create_smart_chunks") return chunks;
        if (command === "save_processing_chunks") {
          processingChunks = chunks.map((chunk) => ({
            meetingId: id,
            ...chunk,
            status: "pending",
            rawSegmentsJson: null,
            errorMsg: null,
            factsStatus: "pending",
            factsJson: null,
            factsErrorMsg: null,
          }));
          return undefined;
        }
        if (command === "get_processing_chunks") return processingChunks;
        if (command === "update_processing_chunk_result") {
          const record = processingChunks.find((chunk) => chunk.index === args.index);
          if (record) {
            record.status = args.status;
            if (args.rawSegmentsJson) record.rawSegmentsJson = args.rawSegmentsJson;
            if (args.errorMsg) record.errorMsg = args.errorMsg;
          }
          return undefined;
        }
        if (command === "update_processing_chunk_facts") {
          const record = processingChunks.find((chunk) => chunk.index === args.index);
          if (record) {
            record.factsStatus = args.status;
            if (args.factsJson) record.factsJson = args.factsJson;
            if (args.errorMsg) record.factsErrorMsg = args.errorMsg;
          }
          return undefined;
        }
        if (command === "transcribe_chunk_cloudflare") {
          throw new Error(
            "Cloudflare API error 429 Too Many Requests: you have used up your daily free allocation of 10,000 neurons",
          );
        }
        if (
          command === "transcribe_chunk" ||
          command === "transcribe_chunk_deepgram" ||
          command === "transcribe_chunk_local"
        ) {
          return transcriptForOffset(Number(args.offsetSec || 0));
        }
        if (
          command === "diarize_audio_turns_modern_cpu_chunked" ||
          command === "diarize_audio_turns_modern_cpu"
        ) {
          return [
            { start: 0, end: 80, speakerIndex: 0 },
            { start: 120, end: 170, speakerIndex: 1 },
          ];
        }
        if (
          command === "refine_diarization_selectively" ||
          command === "align_speaker_turns_to_transcription" ||
          command === "diarize_transcription_end_to_end"
        ) {
          return diarizedResult();
        }
        if (command === "extract_chunk_facts") {
          const chunkIndex = Number(args.chunkIndex);
          return {
            chunkIndex,
            startSec: Number(args.startSec),
            endSec: Number(args.endSec),
            summary:
              chunkIndex === 0
                ? "Equipe alinhou ajustes no leitor de PDF e saida em planilha."
                : "Responsaveis e prazo foram definidos para finalizar a entrega.",
            topics: chunkIndex === 0 ? ["Leitor de PDF", "Planilha", "Orcamento"] : ["Drive", "Prazo"],
            topicEvidence:
              chunkIndex === 0
                ? [
                    {
                      title: "Leitor de PDF",
                      timestampSec: 4,
                      evidence: "ajustar o leitor de PDF",
                    },
                    {
                      title: "Planilha",
                      timestampSec: 24,
                      evidence: "planilha deve sair no formato final",
                    },
                    {
                      title: "Orcamento",
                      timestampSec: 26,
                      evidence: "orcamento internacional aprovado",
                    },
                  ]
                : [
                    {
                      title: "Drive",
                      timestampSec: 122,
                      evidence: "Caio fica responsavel pelo drive",
                    },
                    {
                      title: "Prazo",
                      timestampSec: 136,
                      evidence: "prazo combinado foi sexta-feira",
                    },
                  ],
            decisions:
              chunkIndex === 0
                ? [
                    {
                      title: "A ata deve registrar a planilha como saida final.",
                      owner: "Caio",
                      timestampSec: 24,
                      evidence: "planilha deve sair no formato final",
                    },
                    {
                      title: "Aprovar orcamento internacional",
                      owner: "Caio",
                      timestampSec: 26,
                      evidence: "orcamento internacional aprovado",
                    },
                  ]
                : [],
            actions:
              chunkIndex === 1
                ? [
                    {
                      task: "Revisar a base de clientes",
                      owner: "Emanuella",
                      deadline: "sexta-feira",
                      timestampSec: 128,
                      evidence: "revisa a base de clientes",
                    },
                    {
                      task: "Contratar fornecedor externo",
                      owner: "Financeiro",
                      deadline: "sexta-feira",
                      timestampSec: 136,
                      evidence: "fornecedor externo foi aprovado por todos",
                    },
                  ]
                : [],
            questions: [],
            risks: chunkIndex === 0 ? ["Falha no processamento de PDF"] : [],
          };
        }
        if (command === "save_transcription") {
          return undefined;
        }
        if (command === "save_benchmark_run") {
          savedBenchmarkRuns.push({
            path: args.path,
            content: JSON.parse(String(args.content ?? "{}")),
          });
          (window as typeof window & { __SAVED_BENCHMARK_RUNS__?: typeof savedBenchmarkRuns })
            .__SAVED_BENCHMARK_RUNS__ = savedBenchmarkRuns;
          return undefined;
        }
        if (command === "generate_ata_from_facts_streaming") {
          const parts = [
            '<div class="header"><h1>Reuniao E2E</h1></div>',
            '<div class="section"><h2>Resumo Executivo</h2><p>Leitor de PDF, planilha e prazo foram alinhados.</p></div>',
            '<div class="section"><h2>Acoes</h2><p>Emanuella revisa a base de clientes ate sexta-feira.</p></div>',
          ];
          let html = "";
          for (const delta of parts) {
            html += delta;
            emit("meeting-minutes://minutes-stream", {
              meetingId: args.meetingId,
              delta,
              done: false,
            });
            await delay(25);
          }
          emit("meeting-minutes://minutes-stream", {
            meetingId: args.meetingId,
            delta: "",
            done: true,
          });
          return html;
        }
        if (command === "save_minutes") {
          savedMinutes.push(args);
          (window as typeof window & { __SAVED_MINUTES__?: typeof savedMinutes })
            .__SAVED_MINUTES__ = savedMinutes;
          await delay(10_000);
          return undefined;
        }
        if (command === "update_meeting_status") return undefined;
        if (command === "upsert_processing_job") {
          processingJobs.push({
            stage: args.stage,
            status: args.status,
            progressPct: args.progressPct,
          });
          (window as typeof window & { __PROCESSING_JOB_UPSERTS__?: typeof processingJobs })
            .__PROCESSING_JOB_UPSERTS__ = processingJobs;
          return undefined;
        }

        throw new Error(`Unhandled mock command: ${command}`);
      },
    };
    (window as typeof window & { __PROCESSING_JOB_UPSERTS__?: typeof processingJobs })
      .__PROCESSING_JOB_UPSERTS__ = processingJobs;
    (window as typeof window & { __SAVED_MINUTES__?: typeof savedMinutes })
      .__SAVED_MINUTES__ = savedMinutes;
    (window as typeof window & { __SAVED_BENCHMARK_RUNS__?: typeof savedBenchmarkRuns })
      .__SAVED_BENCHMARK_RUNS__ = savedBenchmarkRuns;
  }, meetingId);
}

async function expectNoHorizontalOverflow(page: Page) {
  const overflow = await page.evaluate(() => {
    const offenders = Array.from(document.querySelectorAll("body *"))
      .filter((element) => {
        const rect = element.getBoundingClientRect();
        return rect.width > 0 && rect.right > window.innerWidth + 2;
      })
      .slice(0, 5)
      .map((element) => ({
        tag: element.tagName,
        text: element.textContent?.trim().slice(0, 80),
        right: element.getBoundingClientRect().right,
        width: element.getBoundingClientRect().width,
      }));
    return offenders;
  });

  expect(overflow).toEqual([]);
}

async function expectReadableNavigation(page: Page) {
  const narrowLinks = await page.locator("nav a").evaluateAll((links) =>
    links
      .map((link) => ({
        text: link.textContent?.replace(/\s+/g, " ").trim() || "",
        width: link.getBoundingClientRect().width,
      }))
      .filter((link) => link.width > 0 && link.width < 130),
  );

  expect(narrowLinks).toEqual([]);
}

const livePanel = (page: Page) =>
  page.getByRole("region", { name: "Painel ao vivo do processamento" });

async function waitForLivePanelReady(page: Page) {
  const panel = livePanel(page);
  await expect(panel.getByRole("heading", { name: "Transcricao, insights e ata" })).toBeVisible();
  await expect(panel.getByRole("button", { name: /Transcricao\s*4/ })).toBeVisible({
    timeout: 20_000,
  });
  await expect(panel.getByRole("button", { name: /Insights\s*2/ })).toBeVisible();
  await expect(panel.getByRole("button", { name: /Ata\s*1/ })).toBeVisible();
  return panel;
}

test.beforeEach(async ({ page }) => {
  await installTauriMock(page);
});

test("processing live panel renders readable transcript, insights, streamed minutes and logs", async ({
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

  await page.goto(`/processing/${meetingId}`);

  await expect(page.getByRole("heading", { name: "Processando reuniao" })).toBeVisible({
    timeout: 45_000,
  });
  await expectReadableNavigation(page);
  const panel = await waitForLivePanelReady(page);
  await expect(
    page.getByText(
      "Motor ativo: Transcricao: Cloudflare Whisper indisponivel nesta execucao; usando Deepgram Nova-3 como fallback.",
    ),
  ).toBeVisible();

  await panel.getByRole("button", { name: /Transcricao/ }).click();
  const liveContent = panel.getByRole("region", { name: "Conteudo ao vivo" });
  await expect.poll(async () => liveContent.evaluate((element) => element.scrollTop)).toBe(0);
  const transcriptCards = panel.locator("article").filter({ hasText: "Bloco" });
  await expect(transcriptCards).toHaveCount(4);
  await expect(transcriptCards.first()).toContainText("Primeiro precisamos");
  await expect(transcriptCards.first()).toContainText(/Caio|Falante em analise/);
  await expect(transcriptCards.first()).toContainText("00:04");
  await expect(transcriptCards.first()).toContainText("00:09");
  await expect(transcriptCards.nth(1)).toContainText("Depois o time valida os documentos");
  await expect(transcriptCards.nth(1)).toContainText("A planilha deve sair no formato final.");
  await expect(transcriptCards.nth(1)).toContainText("00:18");
  await expect(transcriptCards.nth(1)).toContainText("00:27");
  await expect(transcriptCards.nth(2)).toContainText("Caio fica responsavel pelo drive.");

  await page.screenshot({
    path: testInfo.outputPath("processing-live-transcript.png"),
    fullPage: false,
  });

  await panel.getByRole("button", { name: /Insights/ }).click();
  await expect(panel.getByText("Equipe alinhou ajustes no leitor de PDF")).toBeVisible();
  await expect(panel.getByText("A ata deve registrar a planilha")).toBeVisible();
  await expect(panel.getByText("Revisar a base de clientes")).toBeVisible();

  await panel.getByRole("button", { name: /^Ata/ }).click();
  await expect(panel.getByText("Reuniao E2E")).toBeVisible();
  await expect(panel.getByText("Leitor de PDF, planilha e prazo foram alinhados.")).toBeVisible();

  await page.screenshot({
    path: testInfo.outputPath("processing-live-minutes.png"),
    fullPage: false,
  });

  await panel.getByRole("button", { name: /Logs tecnicos/ }).click();
  await expect(panel.getByText("Processamento iniciado.")).toBeVisible();
  await expect(panel.getByText(/Cloudflare Whisper indisponivel/)).toBeVisible();
  await expect(panel.getByText(/usando fallback Deepgram Nova-3/).first()).toBeVisible();
  await expect(
    panel.getByText(
      "Purge anti-alucinacao removeu 1 topico, 1 decisao e 1 acao sem evidencia na transcricao.",
    ),
  ).toBeVisible();
  await expect(panel.getByText("Ata final recebida.")).toBeVisible();
  await expect(panel.getByText("Ata final gerada.")).toBeVisible();

  await expectNoHorizontalOverflow(page);
  await expectReadableNavigation(page);
  expect(consoleMessages).toEqual([]);

  await page.screenshot({
    path: testInfo.outputPath("processing-live-desktop.png"),
    fullPage: false,
  });

  await expect
    .poll(
      async () =>
        page.evaluate(() => {
          const jobs =
            (window as typeof window & {
              __PROCESSING_JOB_UPSERTS__?: Array<Record<string, unknown>>;
            }).__PROCESSING_JOB_UPSERTS__ ?? [];
          const latestByStage = new Map<string, Record<string, unknown>>();
          for (const job of jobs) latestByStage.set(String(job.stage), job);
          return Array.from(latestByStage.entries()).map(([stage, job]) => ({
            stage,
            status: job.status,
            progressPct: job.progressPct,
          }));
        }),
      { timeout: 20_000 },
    )
    .toEqual(
      expect.arrayContaining([
        { stage: "prepare_audio", status: "done", progressPct: 100 },
        { stage: "detect_speech", status: "done", progressPct: 100 },
        { stage: "create_chunks", status: "done", progressPct: 100 },
        { stage: "transcribe", status: "done", progressPct: 100 },
        { stage: "diarize", status: "done", progressPct: 100 },
        { stage: "extract_facts", status: "done", progressPct: 100 },
        { stage: "generate", status: "done", progressPct: 100 },
        { stage: "complete", status: "done", progressPct: 100 },
      ]),
    );

  await expect
    .poll(
      async () =>
        page.evaluate(() => {
          const saved =
            (window as typeof window & {
              __SAVED_MINUTES__?: Array<Record<string, unknown>>;
            }).__SAVED_MINUTES__ ?? [];
          return saved.at(-1)?.modelUsed;
        }),
      { timeout: 20_000 },
    )
    .toBe("meeting-minutes-local-v1-balanced");

  await expect
    .poll(
      async () =>
        page.evaluate(() => {
          const saved =
            (window as typeof window & {
              __SAVED_MINUTES__?: Array<Record<string, unknown>>;
            }).__SAVED_MINUTES__ ?? [];
          return saved.at(-1)?.purgeSummary;
        }),
      { timeout: 20_000 },
    )
    .toEqual({
      removedTopics: 1,
      removedDecisions: 1,
      removedActions: 1,
      removedTotal: 3,
    });

  await expect
    .poll(
      async () =>
        page.evaluate(() => {
          const saved =
            (window as typeof window & {
              __SAVED_MINUTES__?: Array<Record<string, unknown>>;
            }).__SAVED_MINUTES__ ?? [];
          const facts = JSON.parse(String(saved.at(-1)?.factsJson ?? "[]")) as Array<{
            topics?: string[];
          }>;
          return facts.flatMap((fact) => fact.topics ?? []);
        }),
      { timeout: 20_000 },
    )
    .toEqual(["Leitor de PDF", "Planilha", "Drive", "Prazo"]);

  await expect
    .poll(
      async () =>
        page.evaluate(() => {
          const saved =
            (window as typeof window & {
              __SAVED_BENCHMARK_RUNS__?: Array<Record<string, unknown>>;
            }).__SAVED_BENCHMARK_RUNS__ ?? [];
          return saved.at(-1)?.content?.cases?.[0]?.metadata?.purgeSummary;
        }),
      { timeout: 20_000 },
    )
    .toEqual({
      removedTopics: 1,
      removedDecisions: 1,
      removedActions: 1,
      removedTotal: 3,
    });
});

test("processing live panel remains usable on mobile viewport", async ({ page }, testInfo) => {
  await page.goto(`/processing/${meetingId}`);

  await expectReadableNavigation(page);
  const panel = await waitForLivePanelReady(page);
  await expect(panel.getByRole("button", { name: /Transcricao/ })).toBeVisible();

  await panel.getByRole("button", { name: /Transcricao/ }).click();
  const liveContent = panel.getByRole("region", { name: "Conteudo ao vivo" });
  await expect.poll(async () => liveContent.evaluate((element) => element.scrollTop)).toBe(0);
  await expect(panel.locator("article").filter({ hasText: "Bloco" }).first()).toContainText(
    "Primeiro precisamos ajustar o leitor de PDF.",
  );

  await panel.getByRole("button", { name: /Insights/ }).click();
  await expect(panel.getByText("Equipe alinhou ajustes no leitor de PDF")).toBeVisible();

  await panel.getByRole("button", { name: /^Ata/ }).click();
  await expect(panel.getByText("Reuniao E2E")).toBeVisible();

  await expectNoHorizontalOverflow(page);

  await page.screenshot({
    path: testInfo.outputPath("processing-live-mobile.png"),
    fullPage: false,
  });
});
