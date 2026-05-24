import type { RefObject } from "react";
import type { LiveProcessingState, LiveTab } from "../../lib/liveProcessing";

type LiveTabItem = {
  key: LiveTab;
  label: string;
  count: number;
};

type LiveProcessingPanelProps = {
  liveState: LiveProcessingState;
  liveTab: LiveTab;
  liveTabItems: LiveTabItem[];
  livePanelScrollRef: RefObject<HTMLDivElement>;
  onSelectTab: (tab: LiveTab) => void;
};

function liveLogBadgeClass(level: string) {
  if (level === "success") return "bg-emerald-50 text-emerald-700 ring-emerald-200";
  if (level === "warning") return "bg-amber-50 text-amber-700 ring-amber-200";
  if (level === "error") return "bg-red-50 text-red-700 ring-red-200";
  return "bg-blue-50 text-blue-700 ring-blue-200";
}

export default function LiveProcessingPanel({
  liveState,
  liveTab,
  liveTabItems,
  livePanelScrollRef,
  onSelectTab,
}: LiveProcessingPanelProps) {
  return (
    <section
      aria-label="Painel ao vivo do processamento"
      className="rounded-lg border border-gray-200 bg-white shadow-sm"
    >
      <div className="flex flex-col gap-3 border-b border-gray-100 px-5 py-4 sm:flex-row sm:items-center sm:justify-between">
        <div>
          <p className="text-xs font-semibold uppercase tracking-wide text-blue-600">Ao vivo</p>
          <h3 className="mt-1 text-lg font-semibold text-gray-950">
            Transcricao, insights e ata
          </h3>
        </div>
        <div className="flex flex-wrap gap-2">
          {liveTabItems.map((tab) => {
            const selected = liveTab === tab.key;
            return (
              <button
                key={tab.key}
                type="button"
                aria-pressed={selected}
                onClick={() => onSelectTab(tab.key)}
                className={`rounded-lg px-3 py-2 text-sm font-semibold transition ${
                  selected
                    ? "bg-gray-950 text-white shadow-sm"
                    : "bg-gray-50 text-gray-700 hover:bg-gray-100"
                }`}
              >
                {tab.label}
                <span
                  className={`ml-2 rounded-full px-2 py-0.5 text-xs ${
                    selected ? "bg-white/15 text-white" : "bg-white text-gray-500"
                  }`}
                >
                  {tab.count}
                </span>
              </button>
            );
          })}
        </div>
      </div>

      <div
        ref={livePanelScrollRef}
        role="region"
        aria-label="Conteudo ao vivo"
        className="max-h-[28rem] overflow-y-auto px-5 py-4"
      >
        {liveTab === "transcript" && (
          <div className="space-y-2.5">
            {liveState.transcript.length === 0 ? (
              <p className="rounded-lg bg-gray-50 px-4 py-6 text-center text-sm text-gray-500">
                Aguardando primeiro trecho transcrito.
              </p>
            ) : (
              liveState.transcript.map((item) => (
                <article
                  key={item.id}
                  className="grid gap-3 rounded-lg border border-gray-100 bg-white px-4 py-3 shadow-sm sm:grid-cols-[7rem_1fr]"
                >
                  <div className="flex items-start gap-2 sm:block">
                    <div className="rounded-md bg-blue-50 px-2.5 py-1 text-xs font-semibold tabular-nums text-blue-700">
                      {item.timeLabel}
                      <span className="mx-1 text-blue-300">-</span>
                      {item.endTimeLabel}
                    </div>
                  </div>
                  <div className="min-w-0">
                    <div className="mb-1 flex flex-wrap items-center gap-2">
                      <span className="text-xs font-semibold uppercase tracking-wide text-gray-500">
                        Bloco {item.chunkIndex + 1}
                      </span>
                      <span className="rounded-full bg-gray-100 px-2 py-0.5 text-xs font-medium text-gray-500">
                        {item.segmentCount === 1 ? "1 fala" : `${item.segmentCount} falas`}
                      </span>
                      <span className="rounded-full bg-amber-50 px-2 py-0.5 text-xs font-medium text-amber-700">
                        {item.speaker}
                      </span>
                    </div>
                    <p className="text-[15px] leading-7 text-gray-900">{item.text}</p>
                  </div>
                </article>
              ))
            )}
          </div>
        )}

        {liveTab === "insights" && (
          <div className="space-y-3">
            {liveState.insights.length === 0 ? (
              <p className="rounded-lg bg-gray-50 px-4 py-6 text-center text-sm text-gray-500">
                Aguardando primeiros insights.
              </p>
            ) : (
              liveState.insights.map((item) => (
                <article
                  key={item.id}
                  className="rounded-lg border border-gray-100 bg-gray-50 px-4 py-3"
                >
                  <div className="flex flex-col gap-2 sm:flex-row sm:items-start sm:justify-between">
                    <div>
                      <p className="text-xs font-semibold uppercase tracking-wide text-blue-600">
                        {item.timeLabel} · Chunk {item.chunkIndex + 1}
                      </p>
                      <p className="mt-1 text-sm leading-6 text-gray-800">{item.summary}</p>
                    </div>
                    <div className="flex shrink-0 flex-wrap gap-2 text-xs font-semibold text-gray-600">
                      <span className="rounded-full bg-white px-2 py-1">
                        {item.decisionCount} decisoes
                      </span>
                      <span className="rounded-full bg-white px-2 py-1">
                        {item.actionCount} acoes
                      </span>
                      <span className="rounded-full bg-white px-2 py-1">
                        {item.riskCount} riscos
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
                  {(item.decisions.length > 0 || item.actions.length > 0) && (
                    <div className="mt-3 grid gap-3 md:grid-cols-2">
                      {item.decisions.length > 0 && (
                        <div>
                          <p className="text-xs font-semibold uppercase tracking-wide text-gray-500">
                            Decisoes
                          </p>
                          <ul className="mt-1 space-y-1 text-sm leading-6 text-gray-800">
                            {item.decisions.slice(0, 3).map((decision) => (
                              <li key={decision}>- {decision}</li>
                            ))}
                          </ul>
                        </div>
                      )}
                      {item.actions.length > 0 && (
                        <div>
                          <p className="text-xs font-semibold uppercase tracking-wide text-gray-500">
                            Acoes
                          </p>
                          <ul className="mt-1 space-y-1 text-sm leading-6 text-gray-800">
                            {item.actions.slice(0, 3).map((action) => (
                              <li key={action}>- {action}</li>
                            ))}
                          </ul>
                        </div>
                      )}
                    </div>
                  )}
                </article>
              ))
            )}
          </div>
        )}

        {liveTab === "minutes" && (
          <div className="space-y-4">
            {liveState.finalMinutesText ? (
              <pre className="whitespace-pre-wrap rounded-lg border border-blue-100 bg-blue-50 px-4 py-4 text-sm leading-6 text-gray-900">
                {liveState.finalMinutesText}
              </pre>
            ) : liveState.minutesDraft ? (
              <pre className="whitespace-pre-wrap rounded-lg border border-gray-100 bg-gray-50 px-4 py-4 text-sm leading-6 text-gray-800">
                {liveState.minutesDraft}
              </pre>
            ) : (
              <p className="rounded-lg bg-gray-50 px-4 py-6 text-center text-sm text-gray-500">
                Aguardando fatos para montar a ata.
              </p>
            )}
          </div>
        )}

        {liveTab === "logs" && (
          <div className="space-y-2">
            {liveState.logs.length === 0 ? (
              <p className="rounded-lg bg-gray-50 px-4 py-6 text-center text-sm text-gray-500">
                Aguardando eventos tecnicos.
              </p>
            ) : (
              liveState.logs.map((item) => (
                <div
                  key={item.id}
                  className="flex gap-3 rounded-lg border border-gray-100 bg-gray-50 px-3 py-2 text-sm"
                >
                  <span className="w-14 shrink-0 tabular-nums text-gray-500">
                    {item.timeLabel}
                  </span>
                  <span
                    className={`shrink-0 rounded-full px-2 py-0.5 text-xs font-semibold ring-1 ${liveLogBadgeClass(
                      item.level,
                    )}`}
                  >
                    {item.level}
                  </span>
                  <span className="min-w-0 text-gray-800">{item.message}</span>
                </div>
              ))
            )}
          </div>
        )}
      </div>
    </section>
  );
}
