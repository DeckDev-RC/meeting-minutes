import { expect, test, type Page } from "@playwright/test";

const meetingId = "minutes-review-e2e";

test.describe.configure({ timeout: 60_000 });

async function installReviewMock(page: Page) {
  await page.addInitScript((id) => {
    const original = {
      minuteId: "minute-review-1",
      meetingId: id,
      htmlContent: "<section><h1>Ata revisavel</h1><p>Resumo final.</p></section>",
      pdfPath: null,
      modelUsed: "gemini-2.5-flash",
      participantNames: ["Caio", "Rafaela"],
      userEdited: false,
      createdAt: "2026-05-24T11:00:00Z",
      decisions: [
        {
          id: "decision-1",
          minuteId: "minute-review-1",
          meetingId: id,
          itemIndex: 0,
          chunkIndex: 0,
          title: "Aprovar ata revisavel",
          owner: "Caio",
          timestampSec: 42,
          evidence: "Caio aprovou a ata revisavel",
          evidenceId: "evidence-1",
          createdAt: "2026-05-24T11:00:00Z",
        },
      ],
      actions: [
        {
          id: "action-1",
          minuteId: "minute-review-1",
          meetingId: id,
          itemIndex: 0,
          chunkIndex: 0,
          task: "Revisar evidencias fracas",
          owner: "Rafaela",
          deadline: "sexta-feira",
          timestampSec: 84,
          evidence: "Rafaela revisa as evidencias fracas",
          evidenceId: "evidence-2",
          status: "pending",
          priority: "normal",
          completedAt: null,
          createdAt: "2026-05-24T11:01:00Z",
        },
      ],
      evidences: [
        {
          id: "evidence-1",
          minuteId: "minute-review-1",
          meetingId: id,
          parentType: "decision",
          parentId: "decision-1",
          chunkIndex: 0,
          quote: "Caio aprovou a ata revisavel",
          transcriptExcerpt: "Caio aprovou a ata revisavel.",
          validated: true,
          validationScore: 0.95,
          createdAt: "2026-05-24T11:00:00Z",
        },
        {
          id: "evidence-2",
          minuteId: "minute-review-1",
          meetingId: id,
          parentType: "action",
          parentId: "action-1",
          chunkIndex: 0,
          quote: "Rafaela revisa as evidencias fracas",
          transcriptExcerpt: "Rafaela revisa as evidencias fracas.",
          validated: true,
          validationScore: 0.94,
          createdAt: "2026-05-24T11:01:00Z",
        },
      ],
      versions: [
        {
          id: "version-1",
          minuteId: "minute-review-1",
          meetingId: id,
          versionNo: 1,
          changeReason: null,
          hasSnapshot: false,
          createdAt: "2026-05-24T11:00:00Z",
        },
      ],
    };
    const structuredStorageKey = `meeting-minutes-review:${id}:structured`;
    const snapshotsStorageKey = `meeting-minutes-review:${id}:snapshots`;
    const storedStructured = localStorage.getItem(structuredStorageKey);
    const storedSnapshots = localStorage.getItem(snapshotsStorageKey);
    let structured = storedStructured
      ? (JSON.parse(storedStructured) as typeof original)
      : structuredClone(original);
    const snapshots = storedSnapshots
      ? (JSON.parse(storedSnapshots) as Record<string, typeof original>)
      : {};
    const persistStructured = () => {
      localStorage.setItem(structuredStorageKey, JSON.stringify(structured));
    };
    const persistSnapshots = () => {
      localStorage.setItem(snapshotsStorageKey, JSON.stringify(snapshots));
    };

    window.__MEETING_MINUTES_E2E__ = {
      listen: async () => () => undefined,
      invoke: async (command, args = {}) => {
        if (command === "get_structured_minutes_by_meeting") {
          return args.meetingId === id ? structured : null;
        }
        if (command === "get_minutes_by_meeting") return null;
        if (command === "get_processing_chunks") return [];
        if (command === "get_transcription_by_meeting") return null;
        if (command === "update_minute_action") {
          const versionId = `version-${structured.versions.length + 1}`;
          snapshots[versionId] = structuredClone(structured);
          persistSnapshots();
          structured = {
            ...structured,
            userEdited: true,
            actions: structured.actions.map((action) =>
              action.id === args.actionId ? { ...action, ...args.patch } : action,
            ),
            versions: [
              ...structured.versions,
              {
                id: versionId,
                minuteId: structured.minuteId,
                meetingId: id,
                versionNo: structured.versions.length + 1,
                changeReason: args.reason ?? "Revisao manual da acao",
                hasSnapshot: true,
                createdAt: "2026-05-24T12:00:00Z",
              },
            ],
          };
          persistStructured();
          return undefined;
        }
        if (command === "update_minute_decision") {
          const versionId = `version-${structured.versions.length + 1}`;
          snapshots[versionId] = structuredClone(structured);
          persistSnapshots();
          structured = {
            ...structured,
            userEdited: true,
            decisions: structured.decisions.map((decision) =>
              decision.id === args.decisionId ? { ...decision, ...args.patch } : decision,
            ),
            versions: [
              ...structured.versions,
              {
                id: versionId,
                minuteId: structured.minuteId,
                meetingId: id,
                versionNo: structured.versions.length + 1,
                changeReason: args.reason ?? "Revisao manual da decisao",
                hasSnapshot: true,
                createdAt: "2026-05-24T12:02:00Z",
              },
            ],
          };
          persistStructured();
          return undefined;
        }
        if (command === "update_minute_participants") {
          const versionId = `version-${structured.versions.length + 1}`;
          snapshots[versionId] = structuredClone(structured);
          persistSnapshots();
          structured = {
            ...structured,
            userEdited: true,
            participantNames: args.participantNames,
            versions: [
              ...structured.versions,
              {
                id: versionId,
                minuteId: structured.minuteId,
                meetingId: id,
                versionNo: structured.versions.length + 1,
                changeReason: args.reason ?? "Revisao manual dos participantes",
                hasSnapshot: true,
                createdAt: "2026-05-24T12:03:00Z",
              },
            ],
          };
          persistStructured();
          return undefined;
        }
        if (command === "restore_minute_version") {
          const snapshot = snapshots[String(args.versionId)];
          if (snapshot) {
            structured = {
              ...structuredClone(snapshot),
              userEdited: true,
              versions: [
                ...structured.versions,
                {
                  id: "version-restored",
                  minuteId: structured.minuteId,
                  meetingId: id,
                  versionNo: structured.versions.length + 1,
                  changeReason: "Restaurar versao",
                  hasSnapshot: true,
                  createdAt: "2026-05-24T12:05:00Z",
                },
              ],
            };
            persistStructured();
          }
          return undefined;
        }
        throw new Error(`Unhandled mock command: ${command}`);
      },
    };
  }, meetingId);
}

test.beforeEach(async ({ page }) => {
  await installReviewMock(page);
});

test("minutes review edits an action and restores a previous version", async ({ page }) => {
  await page.goto(`/minutes/${meetingId}`);

  await expect(page.getByRole("heading", { name: "Ata da reuniao" })).toBeVisible({
    timeout: 45_000,
  });
  await page.getByRole("button", { name: /Acoes\s*1/ }).click();

  await page.getByLabel("Tarefa da acao 1").fill("Revisar evidencias confirmadas");
  await page.getByLabel("Status da acao 1").selectOption("done");
  await page.getByRole("button", { name: "Salvar acao 1" }).click();

  await expect(page.getByText("Ata editada")).toBeVisible();
  await expect(page.getByLabel("Tarefa da acao 1")).toHaveValue(
    "Revisar evidencias confirmadas",
  );
  await page.getByRole("button", { name: /Ata\s*1/ }).click();
  await expect(page.locator("#minutes-preview")).toContainText(
    "Revisar evidencias confirmadas",
  );
  await page.getByRole("button", { name: /Acoes\s*1/ }).click();

  await page.reload();
  await page.getByRole("button", { name: /Acoes\s*1/ }).click();
  await expect(page.getByLabel("Tarefa da acao 1")).toHaveValue(
    "Revisar evidencias confirmadas",
  );

  await expect(page.getByRole("heading", { name: "Historico de versoes" })).toBeVisible();
  await expect(page.getByRole("button", { name: /Versoes/ })).toHaveCount(0);
  await page.getByRole("button", { name: "Restaurar versao 2" }).click();
  await page.getByRole("button", { name: /Acoes\s*1/ }).click();

  await expect(page.getByLabel("Tarefa da acao 1")).toHaveValue("Revisar evidencias fracas");
});

test("minutes review edits a decision and updates the active preview", async ({ page }) => {
  await page.goto(`/minutes/${meetingId}`);

  await expect(page.getByRole("heading", { name: "Ata da reuniao" })).toBeVisible({
    timeout: 45_000,
  });
  await page.getByRole("button", { name: /Decisoes\s*1/ }).click();

  await page.getByLabel("Titulo da decisao 1").fill("Aprovar ata revisada com Caio");
  await page.getByLabel("Responsavel da decisao 1").fill("Rafaela");
  await page.getByRole("button", { name: "Salvar decisao 1" }).click();

  await expect(page.getByText("Ata editada")).toBeVisible();
  await expect(page.getByLabel("Titulo da decisao 1")).toHaveValue(
    "Aprovar ata revisada com Caio",
  );
  await expect(page.getByLabel("Responsavel da decisao 1")).toHaveValue("Rafaela");

  await page.getByRole("button", { name: /Ata\s*1/ }).click();
  await expect(page.locator("#minutes-preview")).toContainText(
    "Aprovar ata revisada com Caio",
  );
  await expect(page.getByRole("heading", { name: "Historico de versoes" })).toBeVisible();
  await expect(page.getByRole("button", { name: "Restaurar versao 2" })).toBeVisible();
});

test("minutes review edits participants through a dedicated versioned flow", async ({ page }) => {
  await page.goto(`/minutes/${meetingId}`);

  await expect(page.getByRole("heading", { name: "Ata da reuniao" })).toBeVisible({
    timeout: 45_000,
  });
  await page.getByRole("button", { name: /Participantes\s*2/ }).click();

  await expect(page.getByRole("heading", { name: "Participantes da ata" })).toBeVisible();
  await page.getByLabel("Participantes revisados").fill("Caio\nEmanuella\nRafaela");
  await page.getByRole("button", { name: "Salvar participantes" }).click();

  await expect(page.getByText("Ata editada")).toBeVisible();
  await expect(page.getByLabel("Participantes revisados")).toHaveValue(
    "Caio\nEmanuella\nRafaela",
  );
  await expect(page.getByRole("heading", { name: "Historico de versoes" })).toBeVisible();
  await expect(page.getByRole("button", { name: "Restaurar versao 2" })).toBeVisible();

  await page.getByRole("button", { name: /Ata\s*1/ }).click();
  await expect(page.locator("#minutes-preview")).toContainText("Emanuella");
});
