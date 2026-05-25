import { Link, useLocation, useNavigate } from "react-router-dom";
import { useMemo, type ReactNode } from "react";
import { useMeetingStore } from "../store/meetingStore";
import ThemeToggle from "./ThemeToggle";
import { useGlobalShortcuts } from "../lib/shortcuts";

const navItems = [
  { path: "/upload", label: "Nova Reuniao", hint: "Enviar arquivo" },
  { path: "/history", label: "Historico", hint: "Atas e retomadas" },
  { path: "/settings", label: "Configuracoes", hint: "Chaves de API" },
];

export default function Layout({ children }: { children: ReactNode }) {
  const location = useLocation();
  const navigate = useNavigate();
  const { currentMeetingId, progress, stepStatus, error } = useMeetingStore();
  const hasRunningStep = Object.values(stepStatus).some((status) => status === "running");
  const showProcessingLink =
    !!currentMeetingId && (hasRunningStep || !!error || (progress > 0 && progress < 100));
  const shortcuts = useMemo(
    () => [
      { key: "u", mod: true, handler: () => navigate("/upload") },
      { key: "h", mod: true, handler: () => navigate("/history") },
      { key: ",", mod: true, handler: () => navigate("/settings") },
    ],
    [navigate],
  );
  useGlobalShortcuts(shortcuts);

  return (
    <div className="flex h-screen flex-col bg-[#f5f7fb] md:flex-row">
      <aside className="flex w-full shrink-0 flex-col border-b border-gray-200 bg-white/95 md:w-64 md:border-b-0 md:border-r">
        <div className="border-b border-gray-200 p-4">
          <div className="flex items-center gap-3">
            <div className="flex h-9 w-9 items-center justify-center rounded-lg bg-blue-600 text-sm font-bold text-white shadow-sm">
              MM
            </div>
            <div className="min-w-0 flex-1">
              <h1 className="text-base font-bold text-gray-950">Meeting Minutes AI</h1>
              <p className="text-xs text-gray-500">Transcricao e ata</p>
            </div>
            <ThemeToggle />
          </div>
        </div>
        <nav className="grid grid-cols-2 gap-2 p-3 sm:grid-cols-4 md:block md:flex-1">
          {showProcessingLink && (
            <Link
              to={`/processing/${currentMeetingId}`}
              className={`col-span-2 block rounded-lg border px-4 py-3 text-sm shadow-sm sm:col-span-4 md:mb-3 md:min-w-0 ${
                location.pathname.startsWith("/processing/")
                  ? "border-blue-200 bg-blue-50 text-blue-700"
                  : "border-gray-200 bg-white text-gray-700 hover:bg-gray-50"
              }`}
            >
              <span className="block font-medium">
                {error ? "Processamento com erro" : "Processamento ativo"}
              </span>
              <span className="mt-1 block text-xs text-gray-500">{progress}% concluido</span>
            </Link>
          )}
          {navItems.map((item) => (
            <Link
              key={item.path}
              to={item.path}
              className={`block min-w-0 rounded-lg px-4 py-3 text-sm transition-colors md:mb-1 md:flex-none ${
                location.pathname === item.path
                  ? "bg-gray-950 text-white shadow-sm"
                  : "text-gray-600 hover:bg-gray-100 hover:text-gray-950"
              }`}
            >
              <span className="block truncate font-medium">{item.label}</span>
              <span
                className={`mt-0.5 block truncate text-xs ${
                  location.pathname === item.path ? "text-gray-300" : "text-gray-400"
                }`}
              >
                {item.hint}
              </span>
            </Link>
          ))}
        </nav>
      </aside>
      <main className="flex-1 overflow-auto p-4 md:p-8">{children}</main>
    </div>
  );
}
