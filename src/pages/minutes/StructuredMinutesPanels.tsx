import type {
  StructuredAction,
  StructuredDecision,
  StructuredEvidence,
} from "../../lib/types";

export const formatMinuteTime = (seconds: number) => {
  const safe = Math.max(0, Math.round(Number.isFinite(seconds) ? seconds : 0));
  const hours = Math.floor(safe / 3600);
  const minutes = Math.floor((safe % 3600) / 60);
  const secs = safe % 60;
  if (hours > 0) {
    return `${hours}:${String(minutes).padStart(2, "0")}:${String(secs).padStart(2, "0")}`;
  }
  return `${String(minutes).padStart(2, "0")}:${String(secs).padStart(2, "0")}`;
};

export const formatEvidenceScore = (score: number) =>
  `${Math.round(Math.max(0, Math.min(1, score)) * 100)}%`;

const joinMeta = (...items: Array<string | null | undefined>) =>
  items.map((item) => item?.trim()).filter(Boolean).join(" - ");

function EvidenceQualityBadge({
  validated,
  score,
}: {
  validated: boolean;
  score: number;
}) {
  return (
    <span
      className={`inline-flex rounded-full px-2 py-0.5 text-[11px] font-semibold ${
        validated
          ? "bg-emerald-50 text-emerald-700 ring-1 ring-emerald-100"
          : "bg-amber-50 text-amber-700 ring-1 ring-amber-100"
      }`}
    >
      {validated ? "Verificada" : "Fraca"} - {formatEvidenceScore(score)}
    </span>
  );
}

function EmptyStructuredState({ label }: { label: string }) {
  return (
    <p className="rounded-lg bg-gray-50 px-4 py-6 text-center text-sm text-gray-500">
      Nenhum item estruturado em {label}.
    </p>
  );
}

export function StructuredEvidenceWarning({
  evidences,
}: {
  evidences: StructuredEvidence[];
}) {
  const weakCount = evidences.filter((evidence) => !evidence.validated).length;
  if (weakCount === 0) return null;

  return (
    <div className="rounded-lg border border-amber-200 bg-amber-50 px-4 py-3 text-sm font-medium text-amber-800">
      {weakCount === 1
        ? "1 evidencia precisa de revisao"
        : `${weakCount} evidencias precisam de revisao`}
    </div>
  );
}

export function StructuredDecisionsPanel({
  decisions,
  evidencesById,
}: {
  decisions: StructuredDecision[];
  evidencesById: Map<string, StructuredEvidence>;
}) {
  return (
    <section className="rounded-xl border border-gray-200 bg-white p-4 shadow-sm md:p-6">
      <div className="border-b border-gray-100 pb-4">
        <p className="text-xs font-semibold uppercase tracking-wide text-blue-600">
          Decisoes
        </p>
        <h3 className="mt-1 text-xl font-bold text-gray-950">Decisoes estruturadas</h3>
      </div>

      {decisions.length === 0 ? (
        <div className="mt-4">
          <EmptyStructuredState label="decisoes" />
        </div>
      ) : (
        <ul className="mt-4 space-y-3">
          {decisions.map((decision) => {
            const evidence = decision.evidenceId ? evidencesById.get(decision.evidenceId) : undefined;
            return (
              <li
                key={decision.id}
                className="rounded-lg border border-gray-100 bg-gray-50 px-4 py-3"
              >
                <div className="flex flex-col gap-2 sm:flex-row sm:items-start sm:justify-between">
                  <div>
                    <p className="text-sm font-semibold leading-6 text-gray-950">
                      {decision.title}
                    </p>
                    <p className="mt-1 text-xs leading-5 text-gray-500">
                      {joinMeta(
                        decision.owner ? `Responsavel: ${decision.owner}` : "",
                        `Chunk ${decision.chunkIndex + 1}`,
                        formatMinuteTime(decision.timestampSec),
                      )}
                    </p>
                  </div>
                  {evidence && (
                    <EvidenceQualityBadge
                      validated={evidence.validated}
                      score={evidence.validationScore}
                    />
                  )}
                </div>
                {decision.evidence && (
                  <p className="mt-2 text-xs leading-5 text-gray-600">
                    Evidencia: {decision.evidence}
                  </p>
                )}
              </li>
            );
          })}
        </ul>
      )}
    </section>
  );
}

export function StructuredActionsPanel({
  actions,
  evidencesById,
}: {
  actions: StructuredAction[];
  evidencesById: Map<string, StructuredEvidence>;
}) {
  return (
    <section className="rounded-xl border border-gray-200 bg-white p-4 shadow-sm md:p-6">
      <div className="border-b border-gray-100 pb-4">
        <p className="text-xs font-semibold uppercase tracking-wide text-blue-600">
          Acoes
        </p>
        <h3 className="mt-1 text-xl font-bold text-gray-950">Acoes estruturadas</h3>
      </div>

      {actions.length === 0 ? (
        <div className="mt-4">
          <EmptyStructuredState label="acoes" />
        </div>
      ) : (
        <div className="mt-4 overflow-x-auto">
          <table className="min-w-full divide-y divide-gray-100 text-left text-sm">
            <thead className="bg-gray-50 text-xs font-semibold uppercase tracking-wide text-gray-500">
              <tr>
                <th className="px-3 py-2">Acao</th>
                <th className="px-3 py-2">Responsavel</th>
                <th className="px-3 py-2">Prazo</th>
                <th className="px-3 py-2">Evidencia</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-gray-100">
              {actions.map((action) => {
                const evidence = action.evidenceId ? evidencesById.get(action.evidenceId) : undefined;
                return (
                  <tr key={action.id} className="align-top">
                    <td className="px-3 py-3">
                      <p className="font-semibold text-gray-950">{action.task}</p>
                      <p className="mt-1 text-xs text-gray-500">
                        Chunk {action.chunkIndex + 1} - {formatMinuteTime(action.timestampSec)}
                      </p>
                    </td>
                    <td className="px-3 py-3 text-gray-700">
                      {action.owner ? `Responsavel: ${action.owner}` : "Sem responsavel"}
                    </td>
                    <td className="px-3 py-3 text-gray-700">
                      {action.deadline ? `Prazo: ${action.deadline}` : "Sem prazo"}
                    </td>
                    <td className="px-3 py-3">
                      <p className="max-w-md text-xs leading-5 text-gray-600">{action.evidence}</p>
                      {evidence && (
                        <span className="mt-2 inline-flex">
                          <EvidenceQualityBadge
                            validated={evidence.validated}
                            score={evidence.validationScore}
                          />
                        </span>
                      )}
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      )}
    </section>
  );
}

export function StructuredEvidencesPanel({
  evidences,
}: {
  evidences: StructuredEvidence[];
}) {
  return (
    <section className="rounded-xl border border-gray-200 bg-white p-4 shadow-sm md:p-6">
      <div className="border-b border-gray-100 pb-4">
        <p className="text-xs font-semibold uppercase tracking-wide text-blue-600">
          Evidencias
        </p>
        <h3 className="mt-1 text-xl font-bold text-gray-950">Evidencias da ata</h3>
      </div>

      {evidences.length === 0 ? (
        <div className="mt-4">
          <EmptyStructuredState label="evidencias" />
        </div>
      ) : (
        <ul className="mt-4 space-y-3">
          {evidences.map((evidence) => (
            <li
              key={evidence.id}
              className="rounded-lg border border-gray-100 bg-gray-50 px-4 py-3"
            >
              <div className="flex flex-col gap-2 sm:flex-row sm:items-start sm:justify-between">
                <p className="text-xs font-semibold uppercase tracking-wide text-gray-500">
                  {evidence.parentType === "decision" ? "Decisao" : "Acao"} - Chunk{" "}
                  {evidence.chunkIndex + 1}
                </p>
                <EvidenceQualityBadge
                  validated={evidence.validated}
                  score={evidence.validationScore}
                />
              </div>
              <p className="mt-2 text-sm font-medium leading-6 text-gray-900">
                {evidence.quote}
              </p>
              {evidence.transcriptExcerpt ? (
                <p className="mt-2 rounded-lg bg-white px-3 py-2 text-xs leading-5 text-gray-600">
                  Trecho: {evidence.transcriptExcerpt}
                </p>
              ) : (
                <p className="mt-2 text-xs leading-5 text-amber-700">
                  Sem trecho de transcricao forte o suficiente para confirmar esta evidencia.
                </p>
              )}
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
