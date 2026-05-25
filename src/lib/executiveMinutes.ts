import type {
  StructuredAction,
  StructuredDecision,
  StructuredEvidence,
  StructuredMinutesData,
} from "./types";

export type ExecutiveMinutesOptions = {
  title?: string;
  actionLimit?: number;
  decisionLimit?: number;
};

export type ExecutivePreservationLevel = "ok" | "warning" | "risk";

export type ExecutivePreservationMetric = {
  sourceDecisionCount: number;
  sourceActionCount: number;
  sourceTotal: number;
  auditableDecisionCount: number;
  auditableActionCount: number;
  auditableTotal: number;
  exportedDecisionCount: number;
  exportedActionCount: number;
  exportedTotal: number;
  weakEvidenceTotal: number;
  omittedLowPriorityTotal: number;
  auditableRatio: number;
  exportRatio: number;
  level: ExecutivePreservationLevel;
};

const DEFAULT_ACTION_LIMIT = 10;
const DEFAULT_DECISION_LIMIT = 6;

const BUSINESS_ACTION_TERMS = [
  "ajustar",
  "alinhar",
  "aprovar",
  "atualizar",
  "cobrar",
  "concluir",
  "configurar",
  "corrigir",
  "criar",
  "definir",
  "documentar",
  "enviar",
  "fechar contrato",
  "implementar",
  "priorizar",
  "publicar",
  "revisar",
  "resolver",
  "validar",
];

const UI_NOISE_TERMS = [
  "abrir detalhes",
  "apertar",
  "arrastar",
  "clicar",
  "clica",
  "copiar e colar",
  "dar zoom",
  "fechar e revisar",
  "ir para o lado",
  "marcar aqui",
  "reduzir e voltar",
  "rolar",
  "selecionar botao",
  "voltar para",
];

function escapeHtml(value: string | null | undefined) {
  return (value ?? "")
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&#39;");
}

function foldLatinLower(value: string) {
  return value
    .trim()
    .toLowerCase()
    .normalize("NFD")
    .replace(/[\u0300-\u036f]/g, "")
    .replace(/\s+/g, " ");
}

function compactActionKey(action: StructuredAction) {
  return foldLatinLower(action.task)
    .replace(/[^a-z0-9 ]/g, " ")
    .replace(/\s+/g, " ")
    .trim();
}

function hasUsefulOwner(owner: string | null | undefined) {
  const normalized = foldLatinLower(owner ?? "");
  return Boolean(normalized) && !["a definir", "n/a", "na", "sem responsavel"].includes(normalized);
}

function isLikelyUiNoiseAction(action: StructuredAction) {
  const haystack = foldLatinLower(`${action.task} ${action.evidence}`);
  const hasNoise = UI_NOISE_TERMS.some((term) => haystack.includes(term));
  if (!hasNoise) return false;

  const hasBusinessTerm = BUSINESS_ACTION_TERMS.some((term) => haystack.includes(term));
  return !hasBusinessTerm;
}

function scoreExecutiveAction(action: StructuredAction) {
  let score = 0;
  const haystack = foldLatinLower(`${action.task} ${action.evidence}`);

  if (hasUsefulOwner(action.owner)) score += 4;
  if (foldLatinLower(action.deadline ?? "") && foldLatinLower(action.deadline ?? "") !== "a definir") {
    score += 2;
  }
  if (action.priority === "high") score += 3;
  if (action.evidence.trim()) score += 2;
  if (BUSINESS_ACTION_TERMS.some((term) => haystack.includes(term))) score += 4;
  if (action.task.length > 55) score += 1;
  if (action.status === "done" || action.status === "canceled") score -= 2;

  return score;
}

export function selectExecutiveActions(
  actions: StructuredAction[],
  options: { limit?: number; evidences?: StructuredEvidence[] } = {},
) {
  const limit = Math.max(1, options.limit ?? DEFAULT_ACTION_LIMIT);
  const seen = new Set<string>();

  return trustedActions(actions, options.evidences)
    .filter((action) => action.task.trim())
    .filter((action) => !isLikelyUiNoiseAction(action))
    .map((action) => ({ action, score: scoreExecutiveAction(action) }))
    .sort((left, right) => {
      if (right.score !== left.score) return right.score - left.score;
      return left.action.timestampSec - right.action.timestampSec;
    })
    .filter(({ action }) => {
      const key = compactActionKey(action);
      if (!key || seen.has(key)) return false;
      seen.add(key);
      return true;
    })
    .slice(0, limit)
    .map(({ action }) => action);
}

function selectExecutiveDecisions(
  decisions: StructuredDecision[],
  limit: number,
  evidences?: StructuredEvidence[],
) {
  const seen = new Set<string>();
  return trustedDecisions(decisions, evidences)
    .filter((decision) => decision.title.trim())
    .filter((decision) => {
      const key = foldLatinLower(decision.title);
      if (!key || seen.has(key)) return false;
      seen.add(key);
      return true;
    })
    .slice(0, limit);
}

function formatTime(seconds: number) {
  const safe = Math.max(0, Math.round(Number.isFinite(seconds) ? seconds : 0));
  const hours = Math.floor(safe / 3600);
  const minutes = Math.floor((safe % 3600) / 60);
  const secs = safe % 60;
  if (hours > 0) return `${hours}h ${String(minutes).padStart(2, "0")}m`;
  return `${minutes}m ${String(secs).padStart(2, "0")}s`;
}

function displayValue(value: string | null | undefined, fallback: string) {
  const trimmed = value?.trim();
  return trimmed ? trimmed : fallback;
}

function evidenceById(evidences: StructuredEvidence[] | undefined) {
  return new Map((evidences ?? []).map((evidence) => [evidence.id, evidence]));
}

function hasEvidenceRecords(evidences: StructuredEvidence[] | undefined) {
  return Boolean(evidences && evidences.length > 0);
}

function hasTrustedEvidence(
  evidenceId: string | null | undefined,
  evidenceRecords: Map<string, StructuredEvidence> | null,
) {
  if (!evidenceRecords) return true;
  if (!evidenceId) return false;
  const evidence = evidenceRecords.get(evidenceId);
  return Boolean(evidence?.validated);
}

function trustedDecisions(
  decisions: StructuredDecision[],
  evidences: StructuredEvidence[] | undefined,
) {
  const records = hasEvidenceRecords(evidences) ? evidenceById(evidences) : null;
  return decisions.filter((decision) => hasTrustedEvidence(decision.evidenceId, records));
}

function trustedActions(
  actions: StructuredAction[],
  evidences: StructuredEvidence[] | undefined,
) {
  const records = hasEvidenceRecords(evidences) ? evidenceById(evidences) : null;
  return actions.filter((action) => hasTrustedEvidence(action.evidenceId, records));
}

export function calculateExecutivePreservation(
  structured: StructuredMinutesData,
  options: ExecutiveMinutesOptions = {},
): ExecutivePreservationMetric {
  const decisionLimit = Math.max(1, options.decisionLimit ?? DEFAULT_DECISION_LIMIT);
  const actionLimit = Math.max(1, options.actionLimit ?? DEFAULT_ACTION_LIMIT);
  const decisions = structured.decisions ?? [];
  const actions = structured.actions ?? [];
  const auditableDecisions = trustedDecisions(decisions, structured.evidences);
  const auditableActions = trustedActions(actions, structured.evidences);
  const exportedDecisions = selectExecutiveDecisions(
    decisions,
    decisionLimit,
    structured.evidences,
  );
  const exportedActions = selectExecutiveActions(actions, {
    limit: actionLimit,
    evidences: structured.evidences,
  });

  const sourceDecisionCount = decisions.length;
  const sourceActionCount = actions.length;
  const sourceTotal = sourceDecisionCount + sourceActionCount;
  const auditableDecisionCount = auditableDecisions.length;
  const auditableActionCount = auditableActions.length;
  const auditableTotal = auditableDecisionCount + auditableActionCount;
  const exportedDecisionCount = exportedDecisions.length;
  const exportedActionCount = exportedActions.length;
  const exportedTotal = exportedDecisionCount + exportedActionCount;
  const weakEvidenceTotal = Math.max(0, sourceTotal - auditableTotal);
  const omittedLowPriorityTotal = Math.max(0, auditableTotal - exportedTotal);
  const auditableRatio = sourceTotal === 0 ? 1 : auditableTotal / sourceTotal;
  const exportRatio = auditableTotal === 0 ? 1 : exportedTotal / auditableTotal;
  const level: ExecutivePreservationLevel =
    sourceTotal > 0 && auditableRatio < 0.5 ? "risk" : weakEvidenceTotal > 0 ? "warning" : "ok";

  return {
    sourceDecisionCount,
    sourceActionCount,
    sourceTotal,
    auditableDecisionCount,
    auditableActionCount,
    auditableTotal,
    exportedDecisionCount,
    exportedActionCount,
    exportedTotal,
    weakEvidenceTotal,
    omittedLowPriorityTotal,
    auditableRatio,
    exportRatio,
    level,
  };
}

export function buildExecutiveMinutesHtml(
  structured: StructuredMinutesData,
  options: ExecutiveMinutesOptions = {},
) {
  const title = options.title ?? "Ata Executiva";
  const allDecisions = structured.decisions ?? [];
  const allActions = structured.actions ?? [];
  const participantNames = structured.participantNames ?? [];
  const decisions = selectExecutiveDecisions(
    allDecisions,
    Math.max(1, options.decisionLimit ?? DEFAULT_DECISION_LIMIT),
    structured.evidences,
  );
  const actions = selectExecutiveActions(allActions, {
    limit: options.actionLimit ?? DEFAULT_ACTION_LIMIT,
    evidences: structured.evidences,
  });
  const preservation = calculateExecutivePreservation(structured, options);

  const participants =
    participantNames.length > 0
      ? participantNames
      : Array.from(
          new Set(
            [...decisions.map((item) => item.owner), ...actions.map((item) => item.owner)]
              .map((item) => item?.trim())
              .filter((item): item is string => Boolean(item)),
          ),
        );

  let html = "";
  html += `<div class="header"><p class="eyebrow">Ata executiva</p><h1>${escapeHtml(title)}</h1>`;
  html += `<p class="meta">Versao enxuta para leitura rapida - decisoes e acoes priorizadas.</p>`;
  html += `</div>`;

  html += `<div class="section"><h2>Resumo Executivo</h2><div class="summary-box">`;
  html += `<p>Esta versao prioriza os pontos acionaveis da reuniao: ${decisions.length} decisao(oes) e ${actions.length} acao(oes) relevantes.</p>`;
  if (preservation.weakEvidenceTotal > 0) {
    html += `<p>${preservation.weakEvidenceTotal} item(ns) ficaram em quarentena por evidencia fraca e nao entram nesta exportacao executiva.</p>`;
  }
  if (preservation.omittedLowPriorityTotal > 0) {
    html += `<p>${preservation.omittedLowPriorityTotal} item(ns) auditaveis foram omitidos por duplicidade, baixa relevancia ou limite executivo. Use a ata completa para auditoria.</p>`;
  }
  html += `<p>Preservacao executiva: ${preservation.exportedTotal}/${preservation.sourceTotal} fato(s) estruturados no documento enxuto.</p>`;
  html += `</div></div>`;

  html += `<div class="section"><h2>Participantes</h2><div class="participants-list">`;
  if (participants.length === 0) {
    html += `<span>A definir</span>`;
  } else {
    for (const participant of participants) {
      html += `<span>${escapeHtml(participant)}</span>`;
    }
  }
  html += `</div></div>`;

  html += `<div class="section"><h2>Decisoes Principais</h2>`;
  if (decisions.length === 0) {
    html += `<p>Nenhuma decisao explicita foi identificada para a versao executiva.</p>`;
  } else {
    html += `<ul class="decision-list">`;
    for (const decision of decisions) {
      html += `<li><span class="tag-decision">DECISAO</span><strong>${escapeHtml(decision.title)}</strong>`;
      html += `<p>Responsavel: ${escapeHtml(displayValue(decision.owner, "A definir"))} - Momento: ${escapeHtml(formatTime(decision.timestampSec))}</p>`;
      html += `</li>`;
    }
    html += `</ul>`;
  }
  html += `</div>`;

  html += `<div class="section"><h2>Acoes Executivas</h2>`;
  html += `<table class="table-actions"><thead><tr><th>Acao</th><th>Responsavel</th><th>Prazo</th></tr></thead><tbody>`;
  if (actions.length === 0) {
    html += `<tr><td>Nenhuma acao executiva foi identificada.</td><td>A definir</td><td>A definir</td></tr>`;
  } else {
    for (const action of actions) {
      html += `<tr><td><span class="tag-action">ACAO</span>${escapeHtml(action.task)}</td>`;
      html += `<td>${escapeHtml(displayValue(action.owner, "A definir"))}</td>`;
      html += `<td>${escapeHtml(displayValue(action.deadline, "A definir"))}</td></tr>`;
    }
  }
  html += `</tbody></table></div>`;

  html += `<div class="footer-doc"><span>Meeting Minutes AI</span><span>Exportacao executiva</span></div>`;
  return html;
}

export function buildStandaloneMinutesHtml(bodyHtml: string, title = "Ata de Reuniao", css = "") {
  const style = css.trim() ? `<style>${css}</style>` : "";
  return `<!doctype html><html lang="pt-BR"><head><meta charset="utf-8" /><meta name="viewport" content="width=device-width, initial-scale=1" /><title>${escapeHtml(title)}</title>${style}</head><body><main class="minutes-wrapper">${bodyHtml}</main></body></html>`;
}
