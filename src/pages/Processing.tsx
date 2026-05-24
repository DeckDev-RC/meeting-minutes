import ProgressPipeline from "../components/ProgressPipeline";
import LiveProcessingPanel from "./processing/LiveProcessingPanel";
import { PROFILE_LABELS, useProcessingPipeline } from "./processing/useProcessingPipeline";

export default function Processing() {
  const {
    runProfile,
    stepStatus,
    currentStep,
    progress,
    progressTitle,
    progressDetail,
    progressEta,
    progressSpeed,
    liveState,
    liveTab,
    liveTabItems,
    livePanelScrollRef,
    transcriptCount,
    insightCount,
    logCount,
    liveInsightTotals,
    visibleProcessingNotes,
    error,
    onSelectLiveTab,
    onBackToUpload,
  } = useProcessingPipeline();

  return (
    <div className="mx-auto max-w-5xl space-y-6">
      <header className="flex flex-col gap-3 sm:flex-row sm:items-end sm:justify-between">
        <div>
          <h2 className="text-2xl font-bold text-gray-900">Processando reuniao</h2>
          <p className="mt-2 max-w-2xl text-sm leading-6 text-gray-600">
            Acompanhe o arquivo por etapa. Se voce navegar para outra tela, volte pelo atalho
            de processamento na lateral ou pelo historico.
          </p>
        </div>
        <div className="rounded-lg border border-blue-100 bg-blue-50 px-4 py-3 text-blue-700">
          <p className="text-xs font-medium uppercase tracking-wide">Progresso geral</p>
          <p className="mt-1 text-2xl font-bold tabular-nums">{progress}%</p>
          <p className="mt-1 text-xs font-medium">{PROFILE_LABELS[runProfile]}</p>
        </div>
      </header>
      <ProgressPipeline stepStatus={stepStatus} currentStep={currentStep} />
      {visibleProcessingNotes.length > 0 && (
        <div className="space-y-1 rounded-lg border border-blue-100 bg-blue-50 px-4 py-3 text-sm leading-6 text-blue-900">
          {visibleProcessingNotes.map((note) => (
            <p key={note}>
              <span className="font-semibold">Motor ativo: </span>
              {note}
            </p>
          ))}
        </div>
      )}
      <div className="rounded-lg border border-gray-200 bg-white p-5 shadow-sm">
        <div className="mb-4 flex flex-col gap-4 sm:flex-row sm:items-start sm:justify-between">
          <div className="min-w-0">
            <p className="text-xs font-semibold uppercase tracking-wide text-blue-600">
              Fase atual
            </p>
            <h3 className="mt-1 text-lg font-semibold text-gray-900">{progressTitle}</h3>
            <p className="mt-2 max-w-2xl text-sm leading-6 text-gray-600">{progressDetail}</p>
          </div>
          <div className="shrink-0 rounded-lg bg-gray-50 px-4 py-3 text-left sm:text-right">
            <p className="text-xs font-medium uppercase tracking-wide text-gray-500">Concluido</p>
            <p className="mt-1 text-3xl font-bold tabular-nums text-blue-600">{progress}%</p>
          </div>
        </div>

        {(progressEta || progressSpeed) && (
          <dl className="mb-4 grid gap-2 sm:grid-cols-2">
            {progressEta && (
              <div className="rounded-lg border border-gray-100 bg-gray-50 px-3 py-2">
                <dt className="text-xs font-medium uppercase tracking-wide text-gray-500">
                  Tempo estimado
                </dt>
                <dd className="mt-1 text-sm font-semibold text-gray-800">{progressEta}</dd>
              </div>
            )}
            {progressSpeed && (
              <div className="rounded-lg border border-gray-100 bg-gray-50 px-3 py-2">
                <dt className="text-xs font-medium uppercase tracking-wide text-gray-500">
                  Velocidade
                </dt>
                <dd className="mt-1 text-sm font-semibold text-gray-800">{progressSpeed}</dd>
              </div>
            )}
          </dl>
        )}

        {(transcriptCount > 0 || insightCount > 0 || logCount > 0) && (
          <dl className="mb-4 grid gap-2 sm:grid-cols-5">
            {[
              ["Trechos", transcriptCount],
              ["Insights", insightCount],
              ["Decisoes", liveInsightTotals.decisions],
              ["Acoes", liveInsightTotals.actions],
              ["Riscos", liveInsightTotals.risks],
            ].map(([label, value]) => (
              <div key={label} className="rounded-lg border border-gray-100 bg-gray-50 px-3 py-2">
                <dt className="text-xs font-medium uppercase tracking-wide text-gray-500">
                  {label}
                </dt>
                <dd className="mt-1 text-sm font-semibold tabular-nums text-gray-800">
                  {value}
                </dd>
              </div>
            ))}
          </dl>
        )}

        <div
          className="h-2.5 overflow-hidden rounded-full bg-gray-100"
          role="progressbar"
          aria-valuemin={0}
          aria-valuemax={100}
          aria-valuenow={progress}
          aria-label="Progresso do processamento"
        >
          <div
            className="h-full rounded-full bg-blue-600 transition-all duration-500 ease-out"
            style={{ width: `${progress}%` }}
          />
        </div>
      </div>
      <LiveProcessingPanel
        liveState={liveState}
        liveTab={liveTab}
        liveTabItems={liveTabItems}
        livePanelScrollRef={livePanelScrollRef}
        onSelectTab={onSelectLiveTab}
      />
      {error && (
        <div className="mt-6 p-4 bg-red-50 border border-red-200 rounded-lg">
          <p className="text-sm text-red-700">{error}</p>
          <button
            onClick={onBackToUpload}
            className="mt-3 px-4 py-2 bg-red-600 text-white rounded text-sm hover:bg-red-700"
          >
            Voltar
          </button>
        </div>
      )}
    </div>
  );
}
