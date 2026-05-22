import { useEffect, useMemo, useState } from "react";
import { useParams } from "react-router-dom";
import MinutesPreview from "../components/MinutesPreview";
import ExportButton from "../components/ExportButton";
import { getMinutesByMeeting, getProcessingChunks } from "../lib/tauri";
import type { MeetingAction, MeetingChunkInsights, MeetingDecision } from "../lib/types";

type MinutesTab = "minutes" | "insights";

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
    const parsed = JSON.parse(json) as unknown;
    return isMeetingChunkInsights(parsed) ? parsed : null;
  } catch {
    return null;
  }
};

const joinMeta = (...items: Array<string | null | undefined>) =>
  items.map((item) => item?.trim()).filter(Boolean).join(" · ");

function DecisionItem({ decision }: { decision: MeetingDecision }) {
  return (
    <li className="rounded-lg border border-gray-100 bg-white px-3 py-2">
      <p className="text-sm font-medium leading-6 text-gray-900">{decision.title}</p>
      <p className="mt-1 text-xs leading-5 text-gray-500">
        {joinMeta(decision.owner ? `Responsavel: ${decision.owner}` : "", formatTime(decision.timestampSec))}
      </p>
      {decision.evidence && (
        <p className="mt-1 text-xs leading-5 text-gray-500">Evidencia: {decision.evidence}</p>
      )}
    </li>
  );
}

function ActionItem({ action }: { action: MeetingAction }) {
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
    </li>
  );
}

export default function Minutes() {
  const { id } = useParams<{ id: string }>();
  const [html, setHtml] = useState<string | null>(null);
  const [insights, setInsights] = useState<MeetingChunkInsights[]>([]);
  const [activeTab, setActiveTab] = useState<MinutesTab>("minutes");
  const title = "Ata de Reuniao";

  useEffect(() => {
    if (!id) return;
    loadMinutes(id);
  }, [id]);

  const loadMinutes = async (meetingId: string) => {
    try {
      const [data, chunks] = await Promise.all([
        getMinutesByMeeting(meetingId),
        getProcessingChunks(meetingId).catch(() => []),
      ]);
      if (data) {
        setHtml(data.html_content);
      }
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
    <div className="mx-auto max-w-5xl space-y-6">
      <div className="flex flex-col gap-3 sm:flex-row sm:items-end sm:justify-between">
        <div>
          <h2 className="text-2xl font-bold text-gray-950">Ata da reuniao</h2>
          <p className="mt-2 text-sm leading-6 text-gray-600">
            Revise o conteudo gerado e exporte em PDF quando estiver pronto.
          </p>
        </div>
        {activeTab === "minutes" && <ExportButton title={title} />}
      </div>

      <div className="flex flex-wrap gap-2">
        {[
          { key: "minutes" as const, label: "Ata", count: 1 },
          { key: "insights" as const, label: "Insights", count: insights.length },
        ].map((tab) => {
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

      {activeTab === "minutes" ? (
        <div className="rounded-xl border border-gray-200 bg-white p-4 shadow-sm md:p-8">
          <MinutesPreview html={html} />
        </div>
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
            <dl className="grid grid-cols-2 gap-2 text-sm sm:grid-cols-5">
              {[
                ["Chunks", insights.length],
                ["Topicos", insightTotals.topics],
                ["Decisoes", insightTotals.decisions],
                ["Acoes", insightTotals.actions],
                ["Riscos", insightTotals.risks],
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
              {insights.map((item) => (
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
                            <DecisionItem key={`${decision.timestampSec}:${decision.title}`} decision={decision} />
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
                            <ActionItem key={`${action.timestampSec}:${action.task}`} action={action} />
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
              ))}
            </div>
          )}
        </section>
      )}
    </div>
  );
}
