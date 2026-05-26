import type {
  StructuredAction,
  StructuredActionPatch,
  StructuredDecision,
  StructuredDecisionPatch,
} from "../../lib/types";

export interface DraftState<TDraft extends object> {
  bases: Record<string, TDraft>;
  values: Record<string, TDraft>;
}

function mergeDraft<TDraft extends object>(
  nextBase: TDraft,
  previousBase?: TDraft,
  previousValue?: TDraft,
): TDraft {
  if (!previousBase || !previousValue) {
    return { ...nextBase };
  }

  const nextValue = { ...nextBase };
  for (const key of Object.keys(nextBase) as Array<keyof TDraft>) {
    nextValue[key] = Object.is(previousValue[key], previousBase[key])
      ? nextBase[key]
      : previousValue[key];
  }

  return nextValue;
}

function mergeDraftState<TItem extends { id: string }, TDraft extends object>(
  items: TItem[],
  current: DraftState<TDraft> | undefined,
  toDraft: (item: TItem) => TDraft,
): DraftState<TDraft> {
  const bases: Record<string, TDraft> = {};
  const values: Record<string, TDraft> = {};

  for (const item of items) {
    const nextBase = toDraft(item);
    bases[item.id] = nextBase;
    values[item.id] = mergeDraft(nextBase, current?.bases[item.id], current?.values[item.id]);
  }

  return { bases, values };
}

function decisionToDraft(decision: StructuredDecision): StructuredDecisionPatch {
  return {
    title: decision.title,
    owner: decision.owner,
    timestampSec: decision.timestampSec,
    evidence: decision.evidence,
  };
}

function actionToDraft(action: StructuredAction): StructuredActionPatch {
  return {
    task: action.task,
    owner: action.owner,
    deadline: action.deadline,
    timestampSec: action.timestampSec,
    evidence: action.evidence,
    status: action.status,
    priority: action.priority,
    completedAt: action.completedAt,
  };
}

export function createDecisionDraftState(
  decisions: StructuredDecision[],
): DraftState<StructuredDecisionPatch> {
  return mergeDecisionDraftState(decisions, undefined);
}

export function mergeDecisionDraftState(
  decisions: StructuredDecision[],
  current?: DraftState<StructuredDecisionPatch>,
): DraftState<StructuredDecisionPatch> {
  return mergeDraftState(decisions, current, decisionToDraft);
}

export function createActionDraftState(actions: StructuredAction[]): DraftState<StructuredActionPatch> {
  return mergeActionDraftState(actions, undefined);
}

export function mergeActionDraftState(
  actions: StructuredAction[],
  current?: DraftState<StructuredActionPatch>,
): DraftState<StructuredActionPatch> {
  return mergeDraftState(actions, current, actionToDraft);
}
