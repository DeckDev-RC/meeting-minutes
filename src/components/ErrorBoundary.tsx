import { Component, type ErrorInfo, type ReactNode } from "react";

interface Props {
  children: ReactNode;
}

interface State {
  hasError: boolean;
}

export default class ErrorBoundary extends Component<Props, State> {
  state: State = { hasError: false };

  static getDerivedStateFromError(): State {
    return { hasError: true };
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    console.error("Uncaught render error:", error, info);
  }

  render() {
    if (!this.state.hasError) {
      return this.props.children;
    }

    return (
      <div className="flex min-h-screen items-center justify-center bg-gray-50 p-6">
        <div
          role="alert"
          className="max-w-md rounded-lg border border-red-200 bg-white p-6 text-sm text-gray-700 shadow-sm"
        >
          <h1 className="text-lg font-semibold text-gray-950">Algo deu errado</h1>
          <p className="mt-2 leading-6">
            A tela atual falhou ao renderizar. Recarregue o app para voltar a um estado seguro.
          </p>
          <button
            type="button"
            onClick={() => window.location.reload()}
            className="mt-4 rounded-lg bg-gray-950 px-4 py-2 text-sm font-semibold text-white"
          >
            Recarregar
          </button>
        </div>
      </div>
    );
  }
}

