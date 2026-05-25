import { useEffect, useMemo, useState } from "react";
import { useParams } from "react-router-dom";
import MinutesPreview from "../components/MinutesPreview";
import ExportButton from "../components/ExportButton";
import SpeakerMapPanel from "../components/SpeakerMapPanel";
import {
  getMinutesByMeeting,
  getProcessingChunks,
  getStructuredMinutesByMeeting,
  getTranscriptionByMeeting,
  restoreMinuteVersion,
  saveSpeakerMap,
  updateMinuteAction,
  updateMinuteDecision,
  updateMinuteParticipants,
} from "../lib/tauri";
import {
  sanitizeMeetingChunkInsights,
  summarizeEvidenceValidation,
  validateMeetingInsightsEvidence,
  type EvidenceValidationItem,
} from "../lib/minutesEvidence";
import type {
  DiarizedSegment,
  DiarizedResult,
  MeetingAction,
  MeetingChunkInsights,
  MeetingDecision,
  ProcessingChunkRecord,
  StructuredAction,
  StructuredActionPatch,
  StructuredDecision,
  StructuredDecisionPatch,
  StructuredEvidence,
  StructuredMinutesData,
  TranscriptionSegment,
} from "../lib/types";
import {
  applySpeakerMapToText,
  extractSpeakerLabels,
  normalizeSpeakerMap,
  parseSpeakerMapJson,
  type SpeakerMap,
} from "../lib/speakerMap";
import { buildExecutiveMinutesHtml, calculateExecutivePreservation } from "../lib/executiveMinutes";
import {
  StructuredActionsPanel,
  StructuredDecisionsPanel,
  StructuredEvidencesPanel,
  StructuredEvidenceWarning,
  StructuredParticipantsPanel,
  StructuredVersionsPanel,
} from "./minutes/StructuredMinutesPanels";

type MinutesTab =
  | "minutes"
  | "decisions"
  | "actions"
  | "evidences"
  | "participants"
  | "insights"
  | "speakers";

const formatTime = (seconds: number) => {
  const safe = Math.max(0, Math.round(Number.isFinite(seconds) ? seconds : 0));
  const hours = Math.floor(safe / 3600);
  const minutes = Math.floor((safe % 3600) / 60);
  const secs = safe % 60;
  if (hours > 0) {
    return `${hours}:${String(minutes).padStart(2, "0")}:${String(secs).padStart(2, "0")}`;
  }
  return `${String(minutes).padStart(2, "0")}:${String(secs).padStart(2, "0")}`;
};

const isMeetingChunkInsights = (value: unknown): value is MeetingChunkInsights => {
  if (!value || typeof value !== "object") return false;
  const item = value as Partial<MeetingChunkInsights>;
  return (
    typeof item.chunkIndex === "number" &&
    typeof item.startSec === "number" &&
    typeof item.endSec === "number" &&
    typeof item.summary === "string" &&
    Array.isArray(item.topics) &&
    Array.isArray(item.decisions) &&
    Array.isArray(item.actions) &&
    Array.isArray(item.questions) &&
    Array.isArray(item.risks)
  );
};

const parseInsightJson = (json: string | null): MeetingChunkInsights | null => {
  if (!json) return null;
  try {
    const parsed = sanitizeMeetingChunkInsights(JSON.parse(json) as unknown);
    return isMeetingChunkInsights(parsed) ? parsed : null;
  } catch {
    return null;
  }
};

const joinMeta = (...items: Array<string | null | undefined>) =>
  items.map((item) => item?.trim()).filter(Boolean).join(" · ");

const parseStringArrayJson = (json: string | null | undefined): string[] => {
  if (!json) return [];
  try {
    const parsed = JSON.parse(json) as unknown;
    return Array.isArray(parsed) ? parsed.filter((item): item is string => typeof item === "string") : [];
  } catch {
    return [];
  }
};

const parseDiarizedSegmentsJson = (json: string | null | undefined): DiarizedSegment[] => {
  if (!json) return [];
  try {
    const parsed = JSON.parse(json) as Partial<DiarizedResult>;
    if (!Array.isArray(parsed.segments)) return [];
    return parsed.segments.filter((segment): segment is DiarizedSegment => (
      Boolean(segment) &&
      typeof segment.speaker === "string" &&
      typeof segment.start === "number" &&
      typeof segment.end === "number" &&
      typeof segment.text === "string"
    ));
  } catch {
    return [];
  }
};

const parseStoredSegments = (json: string | null): TranscriptionSegment[] => {
  if (!json) return [];
  try {
    const parsed = JSON.parse(json) as unknown;
    if (!Array.isArray(parsed)) return [];
    return parsed
      .map((segment) => {
        if (!segment || typeof segment !== "object") return null;
        const item = segment as Partial<TranscriptionSegment>;
        if (
          typeof item.start !== "number" ||
          typeof item.end !== "number" ||
          typeof item.text !== "string"
        ) {
          return null;
        }
        return {
          id: typeof item.id === "number" ? item.id : 0,
          start: item.start,
          end: item.end,
          text: item.text,
        };
      })
      .filter((segment): segment is TranscriptionSegment => Boolean(segment));
  } catch {
    return [];
  }
};

const escapeHtml = (value: string | null | undefined) =>
  (value ?? "")
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&#39;");

const actionStatusLabel = (status: string) => {
  switch (status) {
    case "in_progress":
      return "Em andamento";
    case "done":
      return "Concluida";
    case "canceled":
      return "Cancelada";
    default:
      return "Pendente";
  }
};

const buildActiveMinutesHtml = (
  baseHtml: string,
  structured: StructuredMinutesData | null,
) => {
  if (!structured?.userEdited) return baseHtml;

  const participants = (structured.participantNames ?? [])
    .map((name) => `<span>${escapeHtml(name)}</span>`)
    .join("");
  const decisions = structured.decisions
    .map(
      (decision) => `
        <li>
          <strong>${escapeHtml(decision.title)}</strong>
          <br />
          <span>${escapeHtml(decision.owner || "Sem responsavel")} - ${escapeHtml(formatTime(decision.timestampSec))}</span>
          <br />
          <em>${escapeHtml(decision.evidence)}</em>
        </li>`,
    )
    .join("");
  const actions = structured.actions
    .map(
      (action) => `
        <li>
          <strong>${escapeHtml(action.task)}</strong>
          <br />
          <span>${escapeHtml(action.owner || "Sem responsavel")} - ${escapeHtml(action.deadline || "Sem prazo")} - ${escapeHtml(actionStatusLabel(action.status))}</span>
          <br />
          <em>${escapeHtml(action.evidence)}</em>
        </li>`,
    )
    .join("");

  return `
    <section>
      <h2>Revisao estruturada ativa</h2>
      <p>Esta ata contem revisoes manuais em participantes, decisoes ou acoes. A exportacao usa estes dados ativos.</p>
      ${participants ? `<h3>Participantes revisados</h3><div class="participants-list">${participants}</div>` : ""}
      ${decisions ? `<h3>Decisoes revisadas</h3><ul>${decisions}</ul>` : ""}
      ${actions ? `<h3>Acoes revisadas</h3><ul>${actions}</ul>` : ""}
      <hr />
    </section>
    ${baseHtml}`;
};

function EvidenceBadge({ validation }: { validation?: EvidenceValidationItem }) {
  if (!validation) return null;
  return (
    <span
      className={`mt-2 inline-flex rounded-full px-2 py-0.5 text-[11px] font-semibold ${
        validation.verified
          ? "bg-emerald-50 text-emerald-700 ring-1 ring-emerald-100"
          : "bg-amber-50 text-amber-700 ring-1 ring-amber-100"
      }`}
    >
      Evidencia {validation.verified ? "verificada" : "fraca"} · {Math.round(validation.score * 100)}%
    </span>
  );
}

function DecisionItem({
  decision,
  validation,
}: {
  decision: MeetingDecision;
  validation?: EvidenceValidationItem;
}) {
  return (
    <li className="rounded-lg border border-gray-100 bg-white px-3 py-2">
      <p className="text-sm font-medium leading-6 text-gray-900">{decision.title}</p>
      <p className="mt-1 text-xs leading-5 text-gray-500">
        {joinMeta(decision.owner ? `Responsavel: ${decision.owner}` : "", formatTime(decision.timestampSec))}
      </p>
      {decision.evidence && (
        <p className="mt-1 text-xs leading-5 text-gray-500">Evidencia: {decision.evidence}</p>
      )}
      <EvidenceBadge validation={validation} />
    </li>
  );
}

function ActionItem({
  action,
  validation,
}: {
  action: MeetingAction;
  validation?: EvidenceValidationItem;
}) {
  return (
    <li className="rounded-lg border border-gray-100 bg-white px-3 py-2">
      <p className="text-sm font-medium leading-6 text-gray-900">{action.task}</p>
      <p className="mt-1 text-xs leading-5 text-gray-500">
        {joinMeta(
          action.owner ? `Responsavel: ${action.owner}` : "",
          action.deadline ? `Prazo: ${action.deadline}` : "",
          formatTime(action.timestampSec),
        )}
      </p>
      {action.evidence && (
        <p className="mt-1 text-xs leading-5 text-gray-500">Evidencia: {action.evidence}</p>
      )}
      <EvidenceBadge validation={validation} />
    </li>
  );
}

function ReviewMetricButton({
  label,
  value,
  detail,
  tone = "default",
  ariaLabel,
  onClick,
}: {
  label: string;
  value: number | string;
  detail: string;
  tone?: "default" | "warning" | "good";
  ariaLabel?: string;
  onClick: () => void;
}) {
  const toneClass =
    tone === "warning"
      ? "border-amber-200 bg-amber-50 text-amber-900 hover:border-amber-300"
      : tone === "good"
        ? "border-emerald-200 bg-emerald-50 text-emerald-900 hover:border-emerald-300"
        : "border-gray-200 bg-white text-gray-900 hover:border-blue-200 hover:bg-blue-50/40";

  return (
    <button
      type="button"
      aria-label={ariaLabel ?? `Abrir ${label}`}
      onClick={onClick}
      className={`rounded-lg border px-4 py-3 text-left shadow-sm transition ${toneClass}`}
    >
      <span className="block text-xs font-semibold uppercase tracking-wide text-gray-500">
        {label}
      </span>
      <span className="mt-1 block text-2xl font-bold tabular-nums">{value}</span>
      <span className="mt-1 block text-xs leading-5 text-gray-600">{detail}</span>
    </button>
  );
}

function CommandButton({
  children,
  onClick,
}: {
  children: string;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className="rounded-lg border border-gray-200 bg-white px-3 py-2 text-sm font-semibold text-gray-800 shadow-sm transition hover:border-blue-200 hover:bg-blue-50"
    >
      {children}
    </button>
  );
}

function ReviewQueuePanel({
  weakEvidences,
  pendingActions,
  topDecisions,
  onSelectTab,
}: {
  weakEvidences: StructuredEvidence[];
  pendingActions: StructuredAction[];
  topDecisions: StructuredDecision[];
  onSelectTab: (tab: MinutesTab) => void;
}) {
  return (
    <aside className="space-y-4 lg:sticky lg:top-6 lg:self-start">
      <section className="rounded-xl border border-gray-200 bg-white p-4 shadow-sm">
        <div className="border-b border-gray-100 pb-3">
          <p className="text-xs font-semibold uppercase tracking-wide text-blue-600">
            Revisao
          </p>
          <h3 className="mt-1 text-lg font-bold text-gray-950">Fila de revisao</h3>
        </div>

        <div className="mt-4 space-y-4">
          <div>
            <button
              type="button"
              onClick={() => onSelectTab("evidences")}
              className="flex w-full items-center justify-between rounded-lg border border-amber-100 bg-amber-50 px-3 py-2 text-left text-sm font-semibold text-amber-900 transition hover:border-amber-200"
            >
              <span>Evidencias fracas</span>
              <span className="rounded-full bg-white px-2 py-0.5 text-xs tabular-nums">
                {weakEvidences.length}
              </span>
            </button>
            {weakEvidences.length > 0 ? (
              <ul className="mt-2 space-y-2">
                {weakEvidences.slice(0, 3).map((evidence) => (
                  <li
                    key={evidence.id}
                    className="rounded-lg border border-amber-100 bg-white px-3 py-2 text-xs leading-5 text-gray-700"
                  >
                    <p className="font-semibold text-gray-900">
                      Chunk {evidence.chunkIndex + 1} · {Math.round(evidence.validationScore * 100)}%
                    </p>
                    <p className="mt-1 line-clamp-3">{evidence.quote}</p>
                  </li>
                ))}
              </ul>
            ) : (
              <p className="mt-2 rounded-lg bg-emerald-50 px-3 py-2 text-xs font-medium text-emerald-700">
                Todas as evidencias estruturadas foram verificadas.
              </p>
            )}
          </div>

          <div>
            <button
              type="button"
              onClick={() => onSelectTab("actions")}
              className="flex w-full items-center justify-between rounded-lg border border-gray-100 bg-gray-50 px-3 py-2 text-left text-sm font-semibold text-gray-900 transition hover:border-blue-200"
            >
              <span>Acoes pendentes</span>
              <span className="rounded-full bg-white px-2 py-0.5 text-xs tabular-nums">
                {pendingActions.length}
              </span>
            </button>
            <ul className="mt-2 space-y-2">
              {pendingActions.slice(0, 4).map((action) => (
                <li
                  key={action.id}
                  className="rounded-lg border border-gray-100 bg-white px-3 py-2 text-xs leading-5 text-gray-700"
                >
                  <p className="font-semibold text-gray-900">{action.task}</p>
                  <p className="mt-1">
                    {joinMeta(action.owner || "Sem responsavel", action.deadline || "Sem prazo")}
                  </p>
                </li>
              ))}
            </ul>
          </div>

          <div>
            <button
              type="button"
              onClick={() => onSelectTab("decisions")}
              className="flex w-full items-center justify-between rounded-lg border border-gray-100 bg-gray-50 px-3 py-2 text-left text-sm font-semibold text-gray-900 transition hover:border-blue-200"
            >
              <span>Decisoes principais</span>
              <span className="rounded-full bg-white px-2 py-0.5 text-xs tabular-nums">
                {topDecisions.length}
              </span>
            </button>
            <ul className="mt-2 space-y-2">
              {topDecisions.slice(0, 3).map((decision) => (
                <li
                  key={decision.id}
                  className="rounded-lg border border-gray-100 bg-white px-3 py-2 text-xs leading-5 text-gray-700"
                >
                  <p className="font-semibold text-gray-900">{decision.title}</p>
                  <p className="mt-1">{joinMeta(decision.owner || "", formatTime(decision.timestampSec))}</p>
                </li>
              ))}
            </ul>
          </div>
        </div>
      </section>
    </aside>
  );
}

export default function Minutes() {
  const { id } = useParams<{ id: string }>();
  const [html, setHtml] = useState<string | null>(null);
  const [structuredMinutes, setStructuredMinutes] = useState<StructuredMinutesData | null>(null);
  const [insights, setInsights] = useState<MeetingChunkInsights[]>([]);
  const [chunks, setChunks] = useState<ProcessingChunkRecord[]>([]);
  const [speakerLabels, setSpeakerLabels] = useState<string[]>([]);
  const [speakerMap, setSpeakerMap] = useState<SpeakerMap>({});
  const [activeTab, setActiveTab] = useState<MinutesTab>("minutes");
  const [reviewError, setReviewError] = useState<string | null>(null);
  const [reviewBusy, setReviewBusy] = useState<string | null>(null);
  const title = "Ata de Reuniao";

  useEffect(() => {
    if (!id) return;
    loadMinutes(id);
  }, [id]);

  const loadMinutes = async (meetingId: string) => {
    try {
      const [structured, data, chunks, transcription] = await Promise.all([
        getStructuredMinutesByMeeting(meetingId).catch(() => null),
        getMinutesByMeeting(meetingId).catch(() => null),
        getProcessingChunks(meetingId).catch(() => []),
        getTranscriptionByMeeting(meetingId).catch(() => null),
      ]);
      setStructuredMinutes(structured);
      setHtml(structured?.htmlContent ?? data?.html_content ?? null);
      setChunks(chunks);
      const labels = extractSpeakerLabels(
        parseStringArrayJson(transcription?.speakers),
        parseDiarizedSegmentsJson(transcription?.diarized),
      );
      setSpeakerLabels(labels);
      setSpeakerMap(normalizeSpeakerMap(labels, parseSpeakerMapJson(transcription?.speaker_map)));
      setInsights(
        chunks
          .map((chunk) => parseInsightJson(chunk.factsJson))
          .filter((item): item is MeetingChunkInsights => Boolean(item))
          .sort((a, b) => a.chunkIndex - b.chunkIndex),
      );
    } catch (err) {
      console.error("Failed to load minutes:", err);
    }
  };

  const insightTotals = useMemo(
    () => ({
      decisions: insights.reduce((sum, item) => sum + item.decisions.length, 0),
      actions: insights.reduce((sum, item) => sum + item.actions.length, 0),
      risks: insights.reduce((sum, item) => sum + item.risks.length, 0),
      questions: insights.reduce((sum, item) => sum + item.questions.length, 0),
      topics: new Set(insights.flatMap((item) => item.topics.map((topic) => topic.trim()).filter(Boolean))).size,
    }),
    [insights],
  );
  const evidenceByChunk = useMemo(() => {
    const chunksByIndex = new Map(chunks.map((chunk) => [chunk.index, chunk]));
    return new Map(
      insights.map((item) => {
        const sourceChunk = chunksByIndex.get(item.chunkIndex);
        return [
          item.chunkIndex,
          validateMeetingInsightsEvidence(item, parseStoredSegments(sourceChunk?.rawSegmentsJson ?? null)),
        ] as const;
      }),
    );
  }, [chunks, insights]);
  const evidenceTotals = useMemo(
    () => summarizeEvidenceValidation(Array.from(evidenceByChunk.values())),
    [evidenceByChunk],
  );
  const activeHtml = useMemo(
    () => (html ? buildActiveMinutesHtml(html, structuredMinutes) : null),
    [html, structuredMinutes],
  );
  const previewHtml = useMemo(
    () => (activeHtml ? applySpeakerMapToText(activeHtml, speakerMap) : null),
    [activeHtml, speakerMap],
  );
  const executiveHtml = useMemo(
    () =>
      structuredMinutes
        ? applySpeakerMapToText(
            buildExecutiveMinutesHtml(structuredMinutes, { title: "Ata Executiva" }),
            speakerMap,
          )
        : null,
    [speakerMap, structuredMinutes],
  );
  const structuredEvidencesById = useMemo(
    () => new Map((structuredMinutes?.evidences ?? []).map((evidence) => [evidence.id, evidence])),
    [structuredMinutes],
  );
  const weakStructuredEvidences = useMemo(
    () => (structuredMinutes?.evidences ?? []).filter((evidence) => !evidence.validated),
    [structuredMinutes],
  );
  const executivePreservation = useMemo(
    () =>
      structuredMinutes
        ? calculateExecutivePreservation(structuredMinutes, {
            title: "Ata Executiva",
          })
        : null,
    [structuredMinutes],
  );
  const pendingStructuredActions = useMemo(
    () =>
      (structuredMinutes?.actions ?? []).filter(
        (action) => action.status !== "done" && action.status !== "canceled",
      ),
    [structuredMinutes],
  );
  const topStructuredDecisions = useMemo(
    () => (structuredMinutes?.decisions ?? []).slice(0, 6),
    [structuredMinutes],
  );
  const hasLegacyOnlyMinutes = Boolean(html && !structuredMinutes);
  const tabs = useMemo(
    () => [
      { key: "minutes" as const, label: "Ata", count: 1 },
      ...(structuredMinutes
        ? [
            {
              key: "decisions" as const,
              label: "Decisoes",
              count: structuredMinutes.decisions.length,
            },
            {
              key: "actions" as const,
              label: "Acoes",
              count: structuredMinutes.actions.length,
            },
            {
              key: "evidences" as const,
              label: "Evidencias",
              count: structuredMinutes.evidences.length,
            },
            {
              key: "participants" as const,
              label: "Participantes",
              count: (structuredMinutes.participantNames ?? []).length,
            },
          ]
        : []),
      ...(insights.length > 0
        ? [{ key: "insights" as const, label: "Insights", count: insights.length }]
        : []),
      { key: "speakers" as const, label: "Falantes", count: speakerLabels.length },
    ],
    [insights.length, speakerLabels.length, structuredMinutes],
  );

  const handleSaveSpeakerMap = async (nextMap: SpeakerMap) => {
    if (!id) return;
    const normalized = normalizeSpeakerMap(speakerLabels, nextMap);
    await saveSpeakerMap(id, normalized);
    setSpeakerMap(normalized);
  };

  const handleUpdateAction = async (actionId: string, patch: StructuredActionPatch) => {
    if (!id) return;
    setReviewBusy(actionId);
    setReviewError(null);
    try {
      await updateMinuteAction(actionId, patch, "Revisao manual da acao");
      await loadMinutes(id);
    } catch (err) {
      setReviewError(err instanceof Error ? err.message : String(err));
    } finally {
      setReviewBusy(null);
    }
  };

  const handleUpdateDecision = async (decisionId: string, patch: StructuredDecisionPatch) => {
    if (!id) return;
    setReviewBusy(decisionId);
    setReviewError(null);
    try {
      await updateMinuteDecision(decisionId, patch, "Revisao manual da decisao");
      await loadMinutes(id);
    } catch (err) {
      setReviewError(err instanceof Error ? err.message : String(err));
    } finally {
      setReviewBusy(null);
    }
  };

  const handleRestoreVersion = async (versionId: string) => {
    if (!id) return;
    setReviewBusy(versionId);
    setReviewError(null);
    try {
      await restoreMinuteVersion(versionId);
      await loadMinutes(id);
    } catch (err) {
      setReviewError(err instanceof Error ? err.message : String(err));
    } finally {
      setReviewBusy(null);
    }
  };

  const handleUpdateParticipants = async (participantNames: string[]) => {
    if (!id) return;
    setReviewBusy("participants");
    setReviewError(null);
    try {
      await updateMinuteParticipants(
        id,
        participantNames,
        "Revisao manual dos participantes",
      );
      await loadMinutes(id);
    } catch (err) {
      setReviewError(err instanceof Error ? err.message : String(err));
    } finally {
      setReviewBusy(null);
    }
  };

  if (!html) {
    return (
      <div className="flex h-64 items-center justify-center">
        <div className="rounded-xl border border-gray-200 bg-white px-6 py-4 text-sm text-gray-500 shadow-sm">
          Carregando ata...
        </div>
      </div>
    );
  }

  return (
    <div className="mx-auto max-w-6xl space-y-6">
      <div className="flex flex-col gap-4 rounded-xl border border-gray-200 bg-white px-5 py-5 shadow-sm lg:flex-row lg:items-start lg:justify-between">
        <div>
          <p className="text-xs font-semibold uppercase tracking-wide text-blue-600">
            Workspace de revisao
          </p>
          <h2 className="text-2xl font-bold text-gray-950">Ata da reuniao</h2>
          <h3 className="mt-2 text-lg font-semibold text-gray-900">Central de revisao</h3>
          <p className="mt-2 max-w-3xl text-sm leading-6 text-gray-600">
            Revise a ata limpa, corrija decisoes e acoes, confira evidencias fracas e acompanhe
            versoes sem perder o documento final.
          </p>
        </div>
        <div className="flex flex-wrap gap-2">
          <CommandButton onClick={() => setActiveTab("evidences")}>
            Revisar evidencias
          </CommandButton>
          <CommandButton onClick={() => setActiveTab("speakers")}>
            Mapear falantes
          </CommandButton>
          {structuredMinutes && (
            <CommandButton onClick={() => setActiveTab("participants")}>
              Participantes
            </CommandButton>
          )}
          {activeTab === "minutes" && <ExportButton title={title} executiveHtml={executiveHtml} />}
        </div>
      </div>

      {structuredMinutes?.userEdited && (
        <div className="inline-flex w-fit rounded-full bg-blue-50 px-3 py-1 text-xs font-semibold text-blue-700 ring-1 ring-blue-100">
          Ata editada
        </div>
      )}

      {reviewError && (
        <div className="rounded-lg border border-red-200 bg-red-50 px-4 py-3 text-sm font-medium text-red-700">
          {reviewError}
        </div>
      )}

      {reviewBusy && (
        <div className="rounded-lg border border-blue-100 bg-blue-50 px-4 py-3 text-sm font-medium text-blue-700">
          Salvando revisao...
        </div>
      )}

      {structuredMinutes && (
        <section
          aria-label="Resumo da revisao"
          className="grid gap-3 sm:grid-cols-2 lg:grid-cols-5"
        >
          <ReviewMetricButton
            label="Decisoes"
            value={structuredMinutes.decisions.length}
            detail="Clique para editar responsaveis, tempo e evidencia."
            ariaLabel="Abrir resumo de decisoes"
            onClick={() => setActiveTab("decisions")}
          />
          <ReviewMetricButton
            label="Acoes"
            value={structuredMinutes.actions.length}
            detail={`${pendingStructuredActions.length} ainda pendentes.`}
            ariaLabel="Abrir resumo de acoes"
            onClick={() => setActiveTab("actions")}
          />
          <ReviewMetricButton
            label="Evidencias fracas"
            value={weakStructuredEvidences.length}
            detail="Itens que merecem conferencia humana."
            tone={weakStructuredEvidences.length > 0 ? "warning" : "good"}
            ariaLabel="Abrir evidencias fracas"
            onClick={() => setActiveTab("evidences")}
          />
          <ReviewMetricButton
            label="Preservacao"
            value={`${executivePreservation?.exportedTotal ?? 0}/${executivePreservation?.sourceTotal ?? 0}`}
            detail={
              executivePreservation?.weakEvidenceTotal
                ? `${executivePreservation.weakEvidenceTotal} em quarentena.`
                : "Exportacao executiva auditavel."
            }
            tone={executivePreservation?.level === "ok" ? "good" : "warning"}
            ariaLabel="Abrir evidencias para revisar preservacao"
            onClick={() => setActiveTab("evidences")}
          />
          <ReviewMetricButton
            label="Versoes"
            value={structuredMinutes.versions.length}
            detail={structuredMinutes.userEdited ? "Edicoes salvas no historico." : "Versao inicial da ata."}
            ariaLabel="Ver historico de versoes"
            onClick={() => setActiveTab("minutes")}
          />
        </section>
      )}

      <div className="flex flex-wrap gap-2" aria-label="Modos de revisao">
        {tabs.map((tab) => {
          const selected = activeTab === tab.key;
          return (
            <button
              key={tab.key}
              type="button"
              aria-pressed={selected}
              onClick={() => setActiveTab(tab.key)}
              className={`rounded-lg px-3 py-2 text-sm font-semibold transition ${
                selected
                  ? "bg-gray-950 text-white shadow-sm"
                  : "bg-white text-gray-700 ring-1 ring-gray-200 hover:bg-gray-50"
              }`}
            >
              {tab.label}
              <span
                className={`ml-2 rounded-full px-2 py-0.5 text-xs ${
                  selected ? "bg-white/15 text-white" : "bg-gray-50 text-gray-500"
                }`}
              >
                {tab.count}
              </span>
            </button>
          );
        })}
      </div>

      <div
        className={
          structuredMinutes
            ? "grid gap-6 lg:grid-cols-[minmax(0,1fr)_21rem]"
            : "space-y-6"
        }
      >
        <div className="min-w-0">
          {activeTab === "minutes" ? (
            <div className="space-y-3">
          {structuredMinutes && <StructuredEvidenceWarning evidences={structuredMinutes.evidences} />}
          {hasLegacyOnlyMinutes && (
            <div className="rounded-lg border border-amber-200 bg-amber-50 px-4 py-3 text-sm font-medium text-amber-800">
              Ata antiga sem estrutura persistida
            </div>
          )}
          <div className="rounded-xl border border-gray-200 bg-white p-4 shadow-sm md:p-8">
            <MinutesPreview html={previewHtml ?? html} />
          </div>
        </div>
          ) : activeTab === "decisions" && structuredMinutes ? (
        <StructuredDecisionsPanel
          decisions={structuredMinutes.decisions}
          evidencesById={structuredEvidencesById}
          onUpdateDecision={handleUpdateDecision}
        />
      ) : activeTab === "actions" && structuredMinutes ? (
        <StructuredActionsPanel
          actions={structuredMinutes.actions}
          evidencesById={structuredEvidencesById}
          onUpdateAction={handleUpdateAction}
        />
      ) : activeTab === "evidences" && structuredMinutes ? (
        <StructuredEvidencesPanel evidences={structuredMinutes.evidences} />
      ) : activeTab === "participants" && structuredMinutes ? (
        <StructuredParticipantsPanel
          participantNames={structuredMinutes.participantNames ?? []}
          onUpdateParticipants={handleUpdateParticipants}
        />
      ) : activeTab === "speakers" ? (
        <SpeakerMapPanel labels={speakerLabels} value={speakerMap} onSave={handleSaveSpeakerMap} />
      ) : (
        <section className="rounded-xl border border-gray-200 bg-white p-4 shadow-sm md:p-6">
          <div className="flex flex-col gap-3 border-b border-gray-100 pb-4 sm:flex-row sm:items-start sm:justify-between">
            <div>
              <p className="text-xs font-semibold uppercase tracking-wide text-blue-600">
                Insights
              </p>
              <h3 className="mt-1 text-xl font-bold text-gray-950">Insights extraidos</h3>
              <p className="mt-2 text-sm leading-6 text-gray-600">
                Decisoes, acoes, riscos e perguntas continuam disponiveis depois da ata.
              </p>
            </div>
            <dl className="grid grid-cols-2 gap-2 text-sm sm:grid-cols-3 lg:grid-cols-6">
              {[
                ["Chunks", insights.length],
                ["Topicos", insightTotals.topics],
                ["Decisoes", insightTotals.decisions],
                ["Acoes", insightTotals.actions],
                ["Riscos", insightTotals.risks],
                ["Evidencias", `${evidenceTotals.verified}/${evidenceTotals.total}`],
              ].map(([label, value]) => (
                <div key={label} className="rounded-lg bg-gray-50 px-3 py-2">
                  <dt className="text-xs font-medium uppercase tracking-wide text-gray-500">
                    {label}
                  </dt>
                  <dd className="mt-1 text-lg font-bold tabular-nums text-gray-950">{value}</dd>
                </div>
              ))}
            </dl>
          </div>

          {insights.length === 0 ? (
            <p className="mt-4 rounded-lg bg-gray-50 px-4 py-6 text-center text-sm text-gray-500">
              Nenhum insight salvo para esta reuniao.
            </p>
          ) : (
            <div className="mt-4 space-y-4">
              {insights.map((item) => {
                const validation = evidenceByChunk.get(item.chunkIndex);
                const validationFor = (
                  kind: EvidenceValidationItem["kind"],
                  label: string,
                  evidence: string,
                ) =>
                  validation?.items.find(
                    (entry) =>
                      entry.kind === kind &&
                      entry.label === label &&
                      entry.evidence === evidence,
                  );
                return (
                <article
                  key={item.chunkIndex}
                  className="rounded-lg border border-gray-100 bg-gray-50 px-4 py-3"
                >
                  <div className="flex flex-col gap-2 sm:flex-row sm:items-start sm:justify-between">
                    <div>
                      <p className="text-xs font-semibold uppercase tracking-wide text-blue-600">
                        {formatTime(item.startSec)} - {formatTime(item.endSec)} · Chunk{" "}
                        {item.chunkIndex + 1}
                      </p>
                      <p className="mt-1 text-sm leading-6 text-gray-800">{item.summary}</p>
                    </div>
                    <div className="flex shrink-0 flex-wrap gap-2 text-xs font-semibold text-gray-600">
                      <span className="rounded-full bg-white px-2 py-1">
                        {item.decisions.length} decisoes
                      </span>
                      <span className="rounded-full bg-white px-2 py-1">
                        {item.actions.length} acoes
                      </span>
                      <span className="rounded-full bg-white px-2 py-1">
                        {item.risks.length} riscos
                      </span>
                    </div>
                  </div>

                  {item.topics.length > 0 && (
                    <div className="mt-3 flex flex-wrap gap-2">
                      {item.topics.map((topic) => (
                        <span
                          key={topic}
                          className="rounded-full border border-blue-100 bg-blue-50 px-2.5 py-1 text-xs font-medium text-blue-700"
                        >
                          {topic}
                        </span>
                      ))}
                    </div>
                  )}

                  <div className="mt-4 grid gap-4 lg:grid-cols-2">
                    {item.decisions.length > 0 && (
                      <div>
                        <p className="text-xs font-semibold uppercase tracking-wide text-gray-500">
                          Decisoes
                        </p>
                        <ul className="mt-2 space-y-2">
                          {item.decisions.map((decision) => (
                            <DecisionItem
                              key={`${decision.timestampSec}:${decision.title}`}
                              decision={decision}
                              validation={validationFor("decision", decision.title, decision.evidence)}
                            />
                          ))}
                        </ul>
                      </div>
                    )}
                    {item.actions.length > 0 && (
                      <div>
                        <p className="text-xs font-semibold uppercase tracking-wide text-gray-500">
                          Acoes
                        </p>
                        <ul className="mt-2 space-y-2">
                          {item.actions.map((action) => (
                            <ActionItem
                              key={`${action.timestampSec}:${action.task}`}
                              action={action}
                              validation={validationFor("action", action.task, action.evidence)}
                            />
                          ))}
                        </ul>
                      </div>
                    )}
                    {item.questions.length > 0 && (
                      <div>
                        <p className="text-xs font-semibold uppercase tracking-wide text-gray-500">
                          Perguntas
                        </p>
                        <ul className="mt-2 space-y-1 text-sm leading-6 text-gray-800">
                          {item.questions.map((question) => (
                            <li key={question}>- {question}</li>
                          ))}
                        </ul>
                      </div>
                    )}
                    {item.risks.length > 0 && (
                      <div>
                        <p className="text-xs font-semibold uppercase tracking-wide text-gray-500">
                          Riscos
                        </p>
                        <ul className="mt-2 space-y-1 text-sm leading-6 text-gray-800">
                          {item.risks.map((risk) => (
                            <li key={risk}>- {risk}</li>
                          ))}
                        </ul>
                      </div>
                    )}
                  </div>
                </article>
                );
              })}
            </div>
          )}
        </section>
          )}
        </div>
        {structuredMinutes && (
          <div className="space-y-4">
            <ReviewQueuePanel
              weakEvidences={weakStructuredEvidences}
              pendingActions={pendingStructuredActions}
              topDecisions={topStructuredDecisions}
              onSelectTab={setActiveTab}
            />
            <StructuredVersionsPanel
              versions={structuredMinutes.versions}
              onRestoreVersion={handleRestoreVersion}
            />
          </div>
        )}
      </div>
    </div>
  );
}
