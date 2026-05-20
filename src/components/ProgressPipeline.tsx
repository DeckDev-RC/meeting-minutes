import type { JobStep } from "../lib/types";

const STEPS: { key: JobStep; label: string; detail: string }[] = [
  {
    key: "extract_audio",
    label: "Preparar audio",
    detail: "Converte e divide em blocos inteligentes.",
  },
  {
    key: "transcribe",
    label: "Transcrever",
    detail: "Processa blocos em paralelo.",
  },
  {
    key: "diarize",
    label: "Organizar falantes",
    detail: "Roda em paralelo e alinha no final.",
  },
  {
    key: "generate",
    label: "Gerar ata",
    detail: "Extrai fatos por bloco e monta o documento.",
  },
];

interface Props {
  stepStatus: Record<JobStep, "pending" | "running" | "done" | "error">;
  currentStep: JobStep | null;
}

const statusLabel = {
  pending: "Pendente",
  running: "Em andamento",
  done: "Concluido",
  error: "Erro",
} as const;

const statusClass = {
  pending: "border-gray-200 bg-white text-gray-500",
  running: "border-blue-300 bg-blue-50 text-blue-700",
  done: "border-green-300 bg-green-50 text-green-700",
  error: "border-red-300 bg-red-50 text-red-700",
} as const;

const dotClass = {
  pending: "bg-gray-300",
  running: "bg-blue-600",
  done: "bg-green-600",
  error: "bg-red-600",
} as const;

export default function ProgressPipeline({ stepStatus, currentStep }: Props) {
  return (
    <section className="rounded-lg border border-gray-200 bg-white p-4 shadow-sm">
      <div className="mb-3 flex items-center justify-between gap-3">
        <div>
          <h3 className="text-sm font-semibold text-gray-900">Etapas do processamento</h3>
          <p className="mt-1 text-xs text-gray-500">
            O estado abaixo continua sendo atualizado enquanto o app estiver aberto.
          </p>
        </div>
      </div>

      <div className="grid gap-3 sm:grid-cols-2 xl:grid-cols-4">
        {STEPS.map((step, index) => {
          const status = stepStatus[step.key];
          const isCurrent = currentStep === step.key;

          return (
            <div
              key={step.key}
              aria-current={isCurrent ? "step" : undefined}
              className={`min-w-0 rounded-lg border p-3 transition-colors ${statusClass[status]} ${
                isCurrent ? "ring-2 ring-blue-100" : ""
              }`}
            >
              <div className="flex items-center gap-2">
                <span className={`h-2.5 w-2.5 shrink-0 rounded-full ${dotClass[status]}`} />
                <span className="text-xs font-medium uppercase tracking-wide">
                  {statusLabel[status]}
                </span>
              </div>
              <div className="mt-2 flex items-start gap-2">
                <span
                  className="flex h-6 w-6 shrink-0 items-center justify-center rounded-full bg-gray-900 text-xs font-semibold text-white"
                  aria-hidden="true"
                >
                  {index + 1}
                </span>
                <div className="min-w-0">
                  <p className="text-sm font-semibold text-gray-900">{step.label}</p>
                  <p className="mt-1 text-xs leading-5 text-gray-600">{step.detail}</p>
                </div>
              </div>
            </div>
          );
        })}
      </div>
    </section>
  );
}
