import { useEffect, useState } from "react";
import type {
  MinuteVersionSummary,
  StructuredAction,
  StructuredActionPatch,
  StructuredDecision,
  StructuredDecisionPatch,
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
  onUpdateDecision,
}: {
  decisions: StructuredDecision[];
  evidencesById: Map<string, StructuredEvidence>;
  onUpdateDecision?: (decisionId: string, patch: StructuredDecisionPatch) => Promise<void>;
}) {
  const [drafts, setDrafts] = useState<Record<string, StructuredDecisionPatch>>({});

  useEffect(() => {
    setDrafts(
      Object.fromEntries(
        decisions.map((decision) => [
          decision.id,
          {
            title: decision.title,
            owner: decision.owner,
            timestampSec: decision.timestampSec,
            evidence: decision.evidence,
          },
        ]),
      ),
    );
  }, [decisions]);

  const updateDraft = (
    decisionId: string,
    patch: Partial<StructuredDecisionPatch>,
  ) => {
    setDrafts((current) => ({
      ...current,
      [decisionId]: {
        ...current[decisionId],
        ...patch,
      },
    }));
  };

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
            const draft = drafts[decision.id] ?? {
              title: decision.title,
              owner: decision.owner,
              timestampSec: decision.timestampSec,
              evidence: decision.evidence,
            };
            return (
              <li
                key={decision.id}
                className="rounded-lg border border-gray-100 bg-gray-50 px-4 py-3"
              >
                <div className="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
                  <div className="min-w-0 flex-1 space-y-3">
                    <div>
                      <label className="text-xs font-semibold uppercase tracking-wide text-gray-500">
                        Titulo
                      </label>
                      <input
                        aria-label={`Titulo da decisao ${decision.itemIndex + 1}`}
                        className="mt-1 w-full rounded-lg border border-gray-200 bg-white px-3 py-2 text-sm font-semibold text-gray-950 outline-none transition focus:border-blue-400 focus:ring-2 focus:ring-blue-100"
                        value={draft.title ?? ""}
                        onChange={(event) => updateDraft(decision.id, { title: event.target.value })}
                        readOnly={!onUpdateDecision}
                      />
                    </div>
                    <div className="grid gap-3 sm:grid-cols-2">
                      <div>
                        <label className="text-xs font-semibold uppercase tracking-wide text-gray-500">
                          Responsavel
                        </label>
                        <input
                          aria-label={`Responsavel da decisao ${decision.itemIndex + 1}`}
                          className="mt-1 w-full rounded-lg border border-gray-200 bg-white px-3 py-2 text-sm text-gray-800 outline-none transition focus:border-blue-400 focus:ring-2 focus:ring-blue-100"
                          value={draft.owner ?? ""}
                          onChange={(event) =>
                            updateDraft(decision.id, { owner: event.target.value || null })
                          }
                          readOnly={!onUpdateDecision}
                        />
                      </div>
                      <div>
                        <label className="text-xs font-semibold uppercase tracking-wide text-gray-500">
                          Tempo
                        </label>
                        <input
                          aria-label={`Tempo da decisao ${decision.itemIndex + 1}`}
                          className="mt-1 w-full rounded-lg border border-gray-200 bg-white px-3 py-2 text-sm text-gray-800 outline-none transition focus:border-blue-400 focus:ring-2 focus:ring-blue-100"
                          type="number"
                          min={0}
                          step={1}
                          value={draft.timestampSec ?? 0}
                          onChange={(event) =>
                            updateDraft(decision.id, {
                              timestampSec: Number(event.target.value) || 0,
                            })
                          }
                          readOnly={!onUpdateDecision}
                        />
                      </div>
                    </div>
                    <div>
                      <label className="text-xs font-semibold uppercase tracking-wide text-gray-500">
                        Evidencia
                      </label>
                      <textarea
                        aria-label={`Evidencia da decisao ${decision.itemIndex + 1}`}
                        className="mt-1 min-h-20 w-full rounded-lg border border-gray-200 bg-white px-3 py-2 text-sm leading-6 text-gray-800 outline-none transition focus:border-blue-400 focus:ring-2 focus:ring-blue-100"
                        value={draft.evidence ?? ""}
                        onChange={(event) => updateDraft(decision.id, { evidence: event.target.value })}
                        readOnly={!onUpdateDecision}
                      />
                    </div>
                    <p className="mt-1 text-xs leading-5 text-gray-500">
                      {joinMeta(
                        `Chunk ${decision.chunkIndex + 1}`,
                        formatMinuteTime(decision.timestampSec),
                      )}
                    </p>
                    <div className="rounded-lg bg-white px-3 py-2 text-xs leading-5 text-gray-600">
                      <p className="text-sm font-semibold text-gray-950">{draft.title}</p>
                      <p>
                        {draft.owner ? `Responsavel: ${draft.owner}` : "Sem responsavel"}
                      </p>
                    </div>
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
                    Evidencia original: {decision.evidence}
                  </p>
                )}
                {onUpdateDecision && (
                  <button
                    type="button"
                    className="mt-3 rounded-lg bg-gray-950 px-3 py-2 text-sm font-semibold text-white shadow-sm transition hover:bg-gray-800"
                    onClick={() => onUpdateDecision(decision.id, draft)}
                  >
                    Salvar decisao {decision.itemIndex + 1}
                  </button>
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
  onUpdateAction,
}: {
  actions: StructuredAction[];
  evidencesById: Map<string, StructuredEvidence>;
  onUpdateAction?: (actionId: string, patch: StructuredActionPatch) => Promise<void>;
}) {
  const [drafts, setDrafts] = useState<Record<string, StructuredActionPatch>>({});

  useEffect(() => {
    setDrafts(
      Object.fromEntries(
        actions.map((action) => [
          action.id,
          {
            task: action.task,
            owner: action.owner,
            deadline: action.deadline,
            timestampSec: action.timestampSec,
            evidence: action.evidence,
            status: action.status,
            priority: action.priority,
            completedAt: action.completedAt,
          },
        ]),
      ),
    );
  }, [actions]);

  const updateDraft = (actionId: string, patch: Partial<StructuredActionPatch>) => {
    setDrafts((current) => ({
      ...current,
      [actionId]: {
        ...current[actionId],
        ...patch,
      },
    }));
  };

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
        <ul className="mt-4 space-y-3">
          {actions.map((action) => {
            const evidence = action.evidenceId ? evidencesById.get(action.evidenceId) : undefined;
            const draft = drafts[action.id] ?? {
              task: action.task,
              owner: action.owner,
              deadline: action.deadline,
              timestampSec: action.timestampSec,
              evidence: action.evidence,
              status: action.status,
              priority: action.priority,
              completedAt: action.completedAt,
            };
            return (
              <li
                key={action.id}
                className="rounded-lg border border-gray-100 bg-gray-50 px-4 py-3"
              >
                <div className="grid gap-3 lg:grid-cols-[1.4fr_0.8fr_0.8fr]">
                  <div>
                    <label className="text-xs font-semibold uppercase tracking-wide text-gray-500">
                      Acao
                    </label>
                    <input
                      aria-label={`Tarefa da acao ${action.itemIndex + 1}`}
                      className="mt-1 w-full rounded-lg border border-gray-200 bg-white px-3 py-2 text-sm font-semibold text-gray-950 outline-none transition focus:border-blue-400 focus:ring-2 focus:ring-blue-100"
                      value={draft.task ?? ""}
                      onChange={(event) => updateDraft(action.id, { task: event.target.value })}
                      readOnly={!onUpdateAction}
                    />
                    <p className="mt-1 text-xs text-gray-500">
                      Chunk {action.chunkIndex + 1} - {formatMinuteTime(action.timestampSec)}
                    </p>
                  </div>
                  <div>
                    <label className="text-xs font-semibold uppercase tracking-wide text-gray-500">
                      Responsavel
                    </label>
                    <input
                      aria-label={`Responsavel da acao ${action.itemIndex + 1}`}
                      className="mt-1 w-full rounded-lg border border-gray-200 bg-white px-3 py-2 text-sm text-gray-800 outline-none transition focus:border-blue-400 focus:ring-2 focus:ring-blue-100"
                      value={draft.owner ?? ""}
                      onChange={(event) =>
                        updateDraft(action.id, { owner: event.target.value || null })
                      }
                      readOnly={!onUpdateAction}
                    />
                  </div>
                  <div>
                    <label className="text-xs font-semibold uppercase tracking-wide text-gray-500">
                      Prazo
                    </label>
                    <input
                      aria-label={`Prazo da acao ${action.itemIndex + 1}`}
                      className="mt-1 w-full rounded-lg border border-gray-200 bg-white px-3 py-2 text-sm text-gray-800 outline-none transition focus:border-blue-400 focus:ring-2 focus:ring-blue-100"
                      value={draft.deadline ?? ""}
                      onChange={(event) =>
                        updateDraft(action.id, { deadline: event.target.value || null })
                      }
                      readOnly={!onUpdateAction}
                    />
                  </div>
                </div>

                <div className="mt-3 grid gap-3 sm:grid-cols-2">
                  <div>
                    <label className="text-xs font-semibold uppercase tracking-wide text-gray-500">
                      Status
                    </label>
                    <select
                      aria-label={`Status da acao ${action.itemIndex + 1}`}
                      className="mt-1 w-full rounded-lg border border-gray-200 bg-white px-3 py-2 text-sm text-gray-800 outline-none transition focus:border-blue-400 focus:ring-2 focus:ring-blue-100"
                      value={draft.status ?? action.status}
                      onChange={(event) =>
                        updateDraft(action.id, {
                          status: event.target.value as StructuredAction["status"],
                          completedAt:
                            event.target.value === "done"
                              ? draft.completedAt ?? new Date().toISOString()
                              : null,
                        })
                      }
                      disabled={!onUpdateAction}
                    >
                      <option value="pending">Pendente</option>
                      <option value="in_progress">Em andamento</option>
                      <option value="done">Concluida</option>
                      <option value="canceled">Cancelada</option>
                    </select>
                  </div>
                  <div>
                    <label className="text-xs font-semibold uppercase tracking-wide text-gray-500">
                      Prioridade
                    </label>
                    <select
                      aria-label={`Prioridade da acao ${action.itemIndex + 1}`}
                      className="mt-1 w-full rounded-lg border border-gray-200 bg-white px-3 py-2 text-sm text-gray-800 outline-none transition focus:border-blue-400 focus:ring-2 focus:ring-blue-100"
                      value={draft.priority ?? action.priority}
                      onChange={(event) =>
                        updateDraft(action.id, {
                          priority: event.target.value as StructuredAction["priority"],
                        })
                      }
                      disabled={!onUpdateAction}
                    >
                      <option value="low">Baixa</option>
                      <option value="normal">Normal</option>
                      <option value="high">Alta</option>
                    </select>
                  </div>
                </div>

                <div className="mt-3">
                  <label className="text-xs font-semibold uppercase tracking-wide text-gray-500">
                    Evidencia
                  </label>
                  <textarea
                    aria-label={`Evidencia da acao ${action.itemIndex + 1}`}
                    className="mt-1 min-h-20 w-full rounded-lg border border-gray-200 bg-white px-3 py-2 text-sm leading-6 text-gray-800 outline-none transition focus:border-blue-400 focus:ring-2 focus:ring-blue-100"
                    value={draft.evidence ?? ""}
                    onChange={(event) => updateDraft(action.id, { evidence: event.target.value })}
                    readOnly={!onUpdateAction}
                  />
                </div>

                {evidence && (
                  <span className="mt-3 inline-flex">
                    <EvidenceQualityBadge
                      validated={evidence.validated}
                      score={evidence.validationScore}
                    />
                  </span>
                )}
                <div className="mt-3 rounded-lg bg-white px-3 py-2 text-xs leading-5 text-gray-600">
                  <p className="text-sm font-semibold text-gray-950">{draft.task}</p>
                  <p>{draft.owner ? `Responsavel: ${draft.owner}` : "Sem responsavel"}</p>
                  <p>{draft.deadline ? `Prazo: ${draft.deadline}` : "Sem prazo"}</p>
                </div>
                {onUpdateAction && (
                  <button
                    type="button"
                    className="mt-3 rounded-lg bg-gray-950 px-3 py-2 text-sm font-semibold text-white shadow-sm transition hover:bg-gray-800"
                    onClick={() => onUpdateAction(action.id, draft)}
                  >
                    Salvar acao {action.itemIndex + 1}
                  </button>
                )}
              </li>
            );
          })}
        </ul>
      )}
    </section>
  );
}

export function StructuredVersionsPanel({
  versions,
  onRestoreVersion,
}: {
  versions: MinuteVersionSummary[];
  onRestoreVersion?: (versionId: string) => Promise<void>;
}) {
  return (
    <section className="rounded-xl border border-gray-200 bg-white p-4 shadow-sm md:p-6">
      <div className="border-b border-gray-100 pb-4">
        <p className="text-xs font-semibold uppercase tracking-wide text-blue-600">
          Versoes
        </p>
        <h3 className="mt-1 text-xl font-bold text-gray-950">Historico de versoes</h3>
      </div>

      {versions.length === 0 ? (
        <div className="mt-4">
          <EmptyStructuredState label="versoes" />
        </div>
      ) : (
        <ul className="mt-4 space-y-3">
          {versions.map((version) => (
            <li
              key={version.id}
              className="flex flex-col gap-3 rounded-lg border border-gray-100 bg-gray-50 px-4 py-3 sm:flex-row sm:items-center sm:justify-between"
            >
              <div>
                <p className="text-sm font-semibold text-gray-950">
                  Versao {version.versionNo}
                </p>
                <p className="mt-1 text-xs leading-5 text-gray-500">
                  {version.changeReason || "Versao inicial"} - {version.createdAt}
                </p>
              </div>
              <button
                type="button"
                className="rounded-lg bg-gray-950 px-3 py-2 text-sm font-semibold text-white shadow-sm transition hover:bg-gray-800 disabled:cursor-not-allowed disabled:bg-gray-200 disabled:text-gray-500"
                disabled={!version.hasSnapshot || !onRestoreVersion}
                onClick={() => onRestoreVersion?.(version.id)}
              >
                Restaurar versao {version.versionNo}
              </button>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}

export function StructuredParticipantsPanel({
  participantNames,
  onUpdateParticipants,
}: {
  participantNames: string[];
  onUpdateParticipants?: (participantNames: string[]) => Promise<void>;
}) {
  const [draft, setDraft] = useState("");

  useEffect(() => {
    setDraft(participantNames.join("\n"));
  }, [participantNames]);

  const parseDraft = () =>
    Array.from(
      new Set(
        draft
          .split(/\r?\n|,/)
          .map((name) => name.trim())
          .filter(Boolean),
      ),
    );

  return (
    <section className="rounded-xl border border-gray-200 bg-white p-4 shadow-sm md:p-6">
      <div className="border-b border-gray-100 pb-4">
        <p className="text-xs font-semibold uppercase tracking-wide text-blue-600">
          Participantes
        </p>
        <h3 className="mt-1 text-xl font-bold text-gray-950">Participantes da ata</h3>
      </div>

      <div className="mt-4 space-y-3">
        <div>
          <label className="text-xs font-semibold uppercase tracking-wide text-gray-500">
            Nomes
          </label>
          <textarea
            aria-label="Participantes revisados"
            className="mt-1 min-h-40 w-full rounded-lg border border-gray-200 bg-white px-3 py-2 text-sm leading-6 text-gray-800 outline-none transition focus:border-blue-400 focus:ring-2 focus:ring-blue-100"
            value={draft}
            onChange={(event) => setDraft(event.target.value)}
            readOnly={!onUpdateParticipants}
          />
        </div>

        {participantNames.length > 0 ? (
          <div className="flex flex-wrap gap-2">
            {participantNames.map((name) => (
              <span
                key={name}
                className="rounded-full border border-blue-100 bg-blue-50 px-2.5 py-1 text-xs font-medium text-blue-700"
              >
                {name}
              </span>
            ))}
          </div>
        ) : (
          <p className="rounded-lg bg-gray-50 px-4 py-3 text-sm text-gray-500">
            Nenhum participante revisado nesta ata.
          </p>
        )}

        {onUpdateParticipants && (
          <button
            type="button"
            className="rounded-lg bg-gray-950 px-3 py-2 text-sm font-semibold text-white shadow-sm transition hover:bg-gray-800"
            onClick={() => onUpdateParticipants(parseDraft())}
          >
            Salvar participantes
          </button>
        )}
      </div>
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
