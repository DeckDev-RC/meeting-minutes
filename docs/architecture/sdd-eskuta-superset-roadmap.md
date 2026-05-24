# Meeting Minutes AI - SDD Para Superar O Eskuta

**Status:** Revisado para execução incremental  
**Data:** 2026-05-23  
**Escopo:** 4 fases de produto, arquitetura, qualidade e empacotamento  
**Referências internas:**  
- `docs/architecture/sidecar-runtime.md`
- `docs/superpowers/plans/2026-05-23-structured-minutes-and-processing-jobs.md`
- `docs/evaluation/*benchmark*.md`

**Decisão desta revisão:** este SDD não autoriza reescrever o motor. Ele define uma evolução incremental que preserva o pico atual do Groq, mantém Cloudflare/Deepgram/local como estratégia adaptativa e só troca runtime/sidecar depois de benchmark comparável em reuniões reais.

## 1. Objetivo

Transformar o `meeting-minutes` em uma solução superior ao Eskuta em todos os eixos relevantes:

1. Motor real de processamento.
2. Qualidade e rastreabilidade da ata.
3. Arquitetura interna.
4. UX de revisão, edição e diagnóstico.
5. Empacotamento para usuário comum.
6. Testes, benchmarks e gates contra regressão.

O produto atual já vence no motor prático: provedores cloud/local, fallback adaptativo, diarização local otimizada, benchmarks reais e instalador funcional. O Eskuta ainda vence em schema, separação de módulos, sidecar e modelo de dados. Este SDD fecha essa diferença sem trocar o que já funciona.

## 2. Definição De "Superar O Eskuta"

| Eixo | Eskuta | Meeting Minutes AI alvo |
| --- | --- | --- |
| Transcrição | Groq/AssemblyAI planejado | Cloudflare, Deepgram, Groq, local, fallback por perfil e cota |
| Diarização | Pyannote opcional | Local chunked, centroid stitching, refinamento seletivo, fallback |
| Ata | Schema Pydantic estruturado | Schema estruturado persistido + HTML/PDF derivado |
| Evidências | `rapidfuzz` no backend | Validação backend + UI acionável + correção seletiva |
| Dados | Tabelas normalizadas | Tabelas normalizadas + compatibilidade com HTML legado |
| Progresso | Status por pipeline | `processing_jobs`, logs, tempos, retomada e diagnóstico |
| UI | Abas básicas | Revisão completa, edição, versão, speaker map, evidências |
| Empacotamento | Sidecar PyInstaller planejado | Instalador único com runtime validado por benchmark |
| Qualidade | Pytest/Vitest | Rust tests, TS build, Playwright, benchmarks E2E e bundle smoke |

O critério final é: tudo que o Eskuta faz bem precisa existir no `meeting-minutes`, mas preservando os diferenciais atuais de velocidade, custo, fallback e qualidade real em reuniões longas.

## 3. Princípios

1. **Sem regressão do motor atual.** Groq, Cloudflare, Deepgram e local continuam disponíveis.
2. **HTML deixa de ser fonte primária.** HTML/PDF passam a ser renderizações de dados estruturados.
3. **Toda afirmação importante precisa de evidência.** Decisões e ações sem evidência ficam marcadas como fracas ou pendentes, não como fato confiável.
4. **Migração compatível.** Bases antigas continuam abrindo; atas antigas continuam visíveis.
5. **Sidecar só substitui runtime atual com benchmark.** Empacotamento elegante não pode piorar tempo, qualidade ou confiabilidade.
6. **Arquivos grandes devem encolher por responsabilidade, não por extração cega.**
7. **Tudo deve ser mensurável.** Cada fase tem testes e critérios objetivos.

## 4. Gate 0 - Baseline E Congelamento

Antes de iniciar qualquer uma das 4 fases, congelar o comportamento atual em um baseline reproduzível. Isso evita "melhorias" que só parecem boas porque mudaram o fluxo medido.

### 4.1 Baselines Conhecidos

| Área | Baseline atual | Fonte |
| --- | --- | --- |
| ASR Cloudflare isolado | `49.04x` tempo real na amostra de 16m27 | `cloud-asr-benchmark-2026-05-22.md` |
| ASR Deepgram isolado | `56.98x` tempo real na amostra de 16m27 | `cloud-asr-benchmark-2026-05-22.md` |
| E2E Cloudflare adaptativo | `72.32s`, `13.65x`, 4 decisões, 19 ações | `cloud-asr-benchmark-2026-05-22.md` |
| E2E Deepgram adaptativo | `73.54s`, `13.42x`, 4 decisões, 15 ações | `cloud-asr-benchmark-2026-05-22.md` |
| Diarização 3h `modern-cpu-chunked` | `307.30s`, `35.72x`, 9 falantes | `diarization-runtime-benchmark-2026-05-23.md` |
| Backend Python 3h `balanced + max 8` | `165.59s`, `66.84x` no backend direto | `diarize-python-profile-study-2026-05-23.md` |
| E2E AMI adaptativo + backend reutilizado | `85.57s`, `14.87x`, 4 falantes | `free-benchmark-protocol.md` |

### 4.2 Regras De Congelamento

- Salvar um `run.json` e um resumo Markdown para cada reunião usada como referência.
- Registrar versão do app, provedor de ASR, perfil de transcrição, runtime de diarização, número esperado de falantes e duração do áudio.
- Não aceitar mudança que reduza velocidade ou qualidade sem justificativa explícita no relatório.
- Groq precisa continuar selecionável e funcional quando houver cota, mesmo que não seja o padrão econômico.
- O app instalado não pode depender do diretório do projeto, `.venv` manual ou variáveis de ambiente de desenvolvimento.

### 4.3 Comandos Mínimos

```powershell
cargo test --manifest-path src-tauri/Cargo.toml
npm run build
npx playwright test tests/e2e/upload-preflight.spec.ts tests/e2e/settings-adaptive-transcription.spec.ts tests/e2e/minutes-insights.spec.ts --project=chromium-desktop
git diff --check
```

## 5. Estado Atual Relevante

Já existe ou acabou de entrar:

- `minute_versions`
- `minute_decisions`
- `minute_actions`
- `minute_evidences`
- `processing_jobs`
- `commands/minutes_validator.rs`
- `commands/audio_chunker.rs`
- `commands/transcription_router.rs`
- `commands/minutes_pipeline.rs`
- `src/pages/processing/LiveProcessingPanel.tsx`
- `src/pages/processing/utils.ts`

Ainda falta:

- comandos de leitura estruturada para `Minutes`;
- UI usando dados estruturados;
- edição/versionamento real;
- diagnóstico visual de jobs;
- refatoração profunda de `db.rs`, `generate.rs`, `Processing.tsx`;
- sidecar único com benchmark de paridade.

## 6. Arquitetura Alvo

### 6.1 Fluxo De Dados

```text
Arquivo de reunião
  -> preparação de áudio/chunks
  -> transcrição adaptativa
  -> diarização local
  -> extração de fatos por chunk
  -> StructuredMinutes
  -> validação de evidências
  -> persistência normalizada
  -> renderização HTML/PDF
  -> revisão/edição/versionamento
```

### 6.2 Fontes De Verdade

| Dado | Fonte primária alvo | Uso |
| --- | --- | --- |
| Transcrição bruta | `transcriptions.raw_whisper` + `processing_chunks.raw_segments_json` | auditoria e retomada |
| Diarização | `transcriptions.diarized` + `speaker_map` | falantes e revisão |
| Fatos extraídos | `processing_chunks.facts_json` | cache por chunk |
| Ata final | `minute_*` tabelas | UI, edição, exportação |
| HTML | `minutes.html_content` | compatibilidade/exportação |
| Histórico | `minute_versions` | auditoria, rollback |
| Progresso | `processing_jobs` | diagnóstico e UX |

### 6.3 Estrutura De Módulos Alvo

Rust:

```text
src-tauri/src/commands/
  db/
    mod.rs
    schema.rs
    meetings.rs
    transcriptions.rs
    chunks.rs
    minutes.rs
    jobs.rs
  generate/
    mod.rs
    gemini.rs
    graph.rs
    owners.rs
    render.rs
    payload.rs
    quality.rs
  audio/
    mod.rs
    chunker.rs
    metadata.rs
    ffmpeg.rs
    cache.rs
  transcribe/
    mod.rs
    router.rs
    cloudflare.rs
    deepgram.rs
    groq.rs
    local.rs
  minutes_validator.rs
```

Frontend:

```text
src/pages/processing/
  Processing.tsx
  useProcessingPipeline.ts
  useLiveProcessing.ts
  LiveProcessingPanel.tsx
  ProcessingSummaryCard.tsx
  ProcessingDiagnostics.tsx
  utils.ts

src/pages/minutes/
  Minutes.tsx
  StructuredMinutesView.tsx
  EvidencePanel.tsx
  ActionsTable.tsx
  DecisionsList.tsx
  VersionHistoryPanel.tsx
  SpeakerReviewPanel.tsx
```

## 7. Matriz Das 4 Fases

| Fase | Gap do Eskuta que fecha | Entrega principal | Gate de sucesso |
| --- | --- | --- | --- |
| 1. Ata estruturada visível | Schema e evidências mais acionáveis | UI consome tabelas `minute_*` | Ata, decisões, ações e evidências abrem sem parse de HTML |
| 2. Edição/versionamento | Histórico e revisão humana | Patches transacionais + `minute_versions` | Editar, recarregar e restaurar sem perda |
| 3. Refatoração profunda | Organização superior | módulos pequenos por domínio | mesmos testes passam e arquivos grandes caem abaixo do alvo |
| 4. Sidecar/runtime | distribuição limpa | sidecar experimental validado | máquina limpa roda sem Python externo e sem regressão |

As fases são sequenciais para produto, mas a Fase 3 pode avançar em cortes pequenos entre Fase 1 e Fase 2 se o corte não mudar comportamento.

## 8. Fase 1 - UI Da Ata Estruturada

### 8.1 Objetivo

Fazer a tela `Minutes` consumir `minute_decisions`, `minute_actions`, `minute_evidences` e `minute_versions`, deixando o HTML como fallback/preview.

### 8.2 Escopo

Criar comandos:

```rust
get_structured_minutes_by_meeting(meeting_id: String) -> Option<StructuredMinutesResponse>
get_minute_evidences(meeting_id: String) -> Vec<MinuteEvidenceResponse>
```

Tipos alvo:

```ts
type StructuredMinutesData = {
  minuteId: string;
  meetingId: string;
  htmlContent: string;
  modelUsed: string;
  createdAt: string;
  decisions: StructuredDecision[];
  actions: StructuredAction[];
  evidences: StructuredEvidence[];
  versions: MinuteVersionSummary[];
};
```

### 8.3 UI

Tela `Minutes` passa a ter:

- Aba `Ata`: HTML atual, mas com aviso se existem evidências fracas.
- Aba `Decisões`: lista estruturada com responsável, momento, evidência e score.
- Aba `Ações`: tabela estruturada com status futuro, responsável e prazo.
- Aba `Evidências`: itens verificados/fracos e trecho usado.
- Aba `Falantes`: manter speaker map atual.

### 8.4 Compatibilidade

Se `get_structured_minutes_by_meeting` retornar vazio:

1. carregar `minutes.html_content`;
2. carregar insights antigos de `processing_chunks.facts_json`;
3. mostrar aviso discreto: "Ata antiga sem estrutura persistida".

### 8.5 Testes

Rust:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml structured_minutes
```

Frontend:

```powershell
npm run build
npx playwright test tests/e2e/minutes-insights.spec.ts --project=chromium-desktop
```

Novos testes Playwright:

- `minutes-structured.spec.ts`
  - mostra decisões estruturadas;
  - mostra ações estruturadas;
  - mostra evidências verificadas/fracas;
  - cai para HTML legado quando não há estrutura.

### 8.6 Critérios De Aceite

- Usuário consegue abrir decisões/ações sem depender de parsing do HTML.
- Evidências fracas aparecem visivelmente.
- Ata antiga continua abrindo.
- Nenhum teste atual quebra.

### 8.7 Fora De Escopo Nesta Fase

- Edição inline.
- Restauração de versões.
- Troca de runtime.
- Mudança no prompt de geração da ata.

## 9. Fase 2 - Edição E Versionamento Real

### 9.1 Objetivo

Permitir que o usuário corrija ata, decisões, ações, responsáveis, prazos e participantes sem perder histórico.

### 9.2 Modelo

Adicionar ou evoluir:

```text
minutes.user_edited INTEGER DEFAULT 0
minute_versions.change_reason TEXT
minute_versions.snapshot_json TEXT
minute_actions.status TEXT DEFAULT 'pending'
minute_actions.priority TEXT DEFAULT 'normal'
minute_actions.completed_at TEXT
```

### 9.3 Comandos

```rust
update_minute_action(action_id, patch)
update_minute_decision(decision_id, patch)
save_minute_revision(meeting_id, reason, structured_payload)
restore_minute_version(version_id)
```

Toda alteração deve:

1. abrir transação;
2. salvar snapshot em `minute_versions`;
3. aplicar patch;
4. marcar `minutes.user_edited = 1`;
5. regenerar HTML/PDF em background ou marcar exportação como desatualizada.

### 9.4 UI

- Modo revisão na tela `Minutes`.
- Edição inline de:
  - título da decisão;
  - responsável;
  - ação;
  - prazo;
  - status;
  - evidência.
- Histórico lateral:
  - versão;
  - data;
  - motivo;
  - restaurar.

### 9.5 Regra De Evidência

Se o usuário editar evidência:

- validar novamente no backend;
- se score baixo, permitir salvar, mas marcar como "evidência manual não confirmada";
- nunca esconder o risco.

### 9.6 Testes

- update de ação cria versão.
- restore de versão volta dados anteriores.
- edição de evidência recalcula score.
- Playwright: editar ação, recarregar página, ver valor persistido.

### 9.7 Critérios De Aceite

- Nenhuma edição destrói a versão anterior.
- Usuário vê quando a ata foi editada.
- HTML/PDF exportados refletem a versão ativa.

### 9.8 Fora De Escopo Nesta Fase

- Regerar a ata automaticamente com outro modelo.
- Sincronização cloud.
- Sistema de permissões multiusuário.

## 10. Fase 3 - Refatoração Profunda Sem Regressão

### 10.1 Objetivo

Reduzir arquivos grandes e tornar o sistema mais sustentável que o Eskuta sem reescrever comportamento.

### 10.2 Alvos De Tamanho

| Arquivo | Atual aproximado | Alvo |
| --- | ---: | ---: |
| `Processing.tsx` | 1800 linhas | < 700 linhas |
| `db.rs` | 1400 linhas | < 400 linhas por módulo |
| `generate.rs` | 1780 linhas | < 500 linhas no módulo principal |
| `audio.rs` | grande | dividido por ffmpeg/cache/chunker |

### 10.3 Ordem Segura

1. Extrair testes antes de lógica quando ainda estiver misturado.
2. Separar `db.rs` por domínio.
3. Separar `generate.rs` por grafo/render/payload/qualidade.
4. Separar `Processing.tsx` em hook de pipeline e componentes visuais.
5. Rodar testes a cada corte.

### 10.4 Regras

- Refactor não muda schema.
- Refactor não muda output de ata.
- Refactor não muda seleção de provedor.
- Refactor não muda diarização.
- Cada PR/commit deve ter teste passando.

### 10.5 Testes

Depois de cada subcorte:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml
npm run build
npx playwright test tests/e2e/minutes-insights.spec.ts --project=chromium-desktop
```

### 10.6 Critérios De Aceite

- `Processing.tsx` abaixo de 700 linhas.
- `db.rs` substituído por `commands/db/*`.
- `generate.rs` abaixo de 500 linhas.
- Todos os testes atuais passam.
- Nenhum fluxo do app instalado muda para o usuário.

### 10.7 Fora De Escopo Nesta Fase

- Otimização algorítmica nova.
- Mudança de provedores.
- Mudança de banco.

## 11. Fase 4 - Sidecar Único Com Benchmark

### 11.1 Objetivo

Ter instalador único para usuário comum, mas com runtime mais limpo que o bundle de venv atual, sem perder performance.

### 11.2 Estratégia

Criar sidecar experimental:

```text
meeting-minutes-sidecar.exe
  command: transcribe-local
  command: diarize-modern-cpu
  command: health
```

Contrato CLI:

```powershell
meeting-minutes-sidecar.exe diarize-modern-cpu --input request.json --output result.json
meeting-minutes-sidecar.exe transcribe-local --input request.json --output result.json
meeting-minutes-sidecar.exe health
```

### 11.3 Request/Response

`diarize-modern-cpu` request:

```json
{
  "audioPath": "C:/path/audio.wav",
  "chunks": [],
  "expectedSpeakers": 4,
  "numThreads": 6,
  "mode": "chunked"
}
```

Response:

```json
{
  "ok": true,
  "turns": [{ "start": 0.0, "end": 4.2, "speakerIndex": 0 }],
  "telemetry": {
    "wallClockSec": 42.1,
    "backend": "sidecar-modern-cpu",
    "modelLoadSec": 3.2
  }
}
```

### 11.4 Benchmark Obrigatório

Comparar:

1. runtime atual bundled venv;
2. sidecar PyInstaller;
3. sidecar em modo one-folder, se one-file for lento;
4. fallback atual.

Amostras:

- reunião curta/problemática;
- reunião de 3h;
- reunião de 4h46;
- amostra 10-20 min.

Métricas:

- tempo total;
- tempo de preparação;
- tempo transcrição;
- tempo diarização;
- tempo geração da ata;
- número de falantes;
- decisões/ações/riscos/perguntas;
- evidências verificadas;
- tamanho do instalador;
- tempo de primeira execução.

### 11.5 Critérios De Troca

Sidecar vira padrão somente se:

- tempo total não piorar mais que 5%;
- qualidade de falantes equivalente;
- decisões/ações não caírem mais que margem aceitável;
- evidências verificadas não piorarem;
- instalador for mais simples ou menor;
- app instalado funcionar em máquina limpa sem Python.

Se falhar, manter runtime atual e documentar causa.

### 11.6 Fora De Escopo Nesta Fase

- Tornar sidecar padrão sem matriz de benchmark.
- Exigir instalação manual de Python/modelos.
- Remover o runtime atual antes de paridade comprovada.

## 12. Migração, Rollback E Compatibilidade

### 12.1 Migração SQLite

- Todas as tabelas novas usam `CREATE TABLE IF NOT EXISTS`.
- Colunas novas usam checagem via `PRAGMA table_info` antes de `ALTER TABLE`.
- Migrations nunca apagam `minutes.html_content`, `transcriptions.diarized` ou `processing_chunks.facts_json`.
- Dados estruturados podem ser reconstruídos a partir de `facts_json` quando disponível.
- Bases antigas sem estrutura continuam abrindo com aviso de compatibilidade.

### 12.2 Rollback Funcional

Se uma fase quebrar:

1. manter leitura de HTML legado;
2. manter `save_minutes` compatível com parâmetros antigos;
3. desativar UI nova por fallback local, não por migração destrutiva;
4. preservar jobs/evidências como diagnóstico, mesmo que a UI estruturada seja ocultada temporariamente.

### 12.3 Contratos Que Não Podem Quebrar

- `save_minutes` precisa continuar salvando HTML final.
- `processing_chunks` precisa continuar servindo retomada.
- `transcriptions` precisa continuar salvando segmentos diarizados.
- Settings precisa continuar salvando Groq, Cloudflare, Deepgram e Gemini no keyring.
- Exportação PDF/HTML precisa continuar disponível.

## 13. Segurança E Credenciais

- API keys continuam no keyring.
- Nenhuma chave em DB, logs, benchmark ou docs.
- `processing_jobs.error_msg` deve sanitizar mensagens antes de persistir quando vierem de provedores.
- Exportações não devem incluir tokens.
- Testes não devem depender de chaves reais.
- Mensagens de erro 429/401 de provedores devem ser resumidas antes de persistir, porque algumas APIs ecoam metadados de organização, conta ou billing.

## 14. Observabilidade E Diagnóstico

Adicionar tela/painel:

- provedores configurados;
- cota Cloudflare marcada como esgotada;
- Deepgram configurado;
- Groq configurado/opcional;
- local runtime disponível;
- jobs por reunião;
- tempo por etapa;
- último erro por etapa;
- caminho do benchmark run.

Critério: usuário e dev conseguem entender por que uma reunião demorou ou falhou sem abrir DevTools.

## 15. Plano De Testes Sem Regressão

### 15.1 Gates Locais Obrigatórios

```powershell
cargo test --manifest-path src-tauri/Cargo.toml
npm run build
npx playwright test tests/e2e/upload-preflight.spec.ts tests/e2e/settings-adaptive-transcription.spec.ts tests/e2e/minutes-insights.spec.ts --project=chromium-desktop
```

### 15.2 Gates De Benchmark

Antes de trocar runtime ou algoritmo:

```powershell
npm run benchmark:audio-prepare -- <arquivo>
npm run compare:diarization -- <baseline.json> <current.json>
```

Para E2E:

```powershell
cargo run --manifest-path src-tauri/Cargo.toml --bin e2e_benchmark -- --input "<arquivo>"
```

### 15.3 Critérios De Bloqueio

Bloquear merge/build se:

- teste Rust falhar;
- build TS falhar;
- Playwright smoke falhar;
- mudança reduzir evidências verificadas em benchmark crítico sem explicação;
- app instalado depender de diretório do projeto;
- Groq deixar de funcionar como opção.

## 16. Definition Of Done Por Fase

| Fase | Definition of Done |
| --- | --- |
| 1 | UI abre ata estruturada, evidências e fallback legado; Rust, TS e Playwright passam |
| 2 | edição gera versão, restore funciona, HTML/PDF refletem versão ativa |
| 3 | arquivos-alvo abaixo do limite, sem mudança perceptível no app instalado |
| 4 | sidecar passa benchmark, app roda em máquina limpa, runtime antigo ainda é fallback |

## 17. Riscos

| Risco | Mitigação |
| --- | --- |
| Migração DB quebrar base existente | testes com DB legado e `CREATE TABLE IF NOT EXISTS` |
| UI estruturada divergir do HTML | renderizar HTML a partir do mesmo `StructuredMinutes` |
| Sidecar piorar startup | benchmark one-file vs one-folder |
| Refactor quebrar pipeline longo | cortes pequenos e Playwright smoke |
| Evidência fuzzy aceitar falso positivo | mostrar score e permitir revisão manual |
| Arquivos continuarem grandes | metas de linha por fase e bloqueio por revisão |
| Baseline ficar obsoleto | renovar benchmark quando mudar versão do app, modelo ou runtime |
| Sidecar one-file atrasar primeira execução | comparar one-file e one-folder antes de escolher bundle |

## 18. Ordem De Execução Recomendada

### Sprint A - Ata Estruturada Visível

1. Criar comandos de leitura estruturada.
2. Criar tipos TS.
3. Adaptar `Minutes` para dados estruturados.
4. Mostrar evidências e scores.
5. Playwright de ata estruturada.

### Sprint B - Edição E Versões

1. Criar comandos de patch.
2. Criar snapshot/versionamento.
3. Criar UI de edição.
4. Criar histórico e restore.
5. Testes de edição/restore.

### Sprint C - Refatoração

1. Quebrar `db.rs`.
2. Quebrar `generate.rs`.
3. Quebrar `Processing.tsx`.
4. Revalidar smoke e build.

### Sprint D - Sidecar

1. Criar sidecar experimental.
2. Empacotar em build alternativo.
3. Rodar matriz de benchmark.
4. Decidir default com dados.

## 19. Entrega Final Esperada

Ao concluir as 4 fases:

- o app instalado processa reuniões longas sem depender de cota única;
- a ata é auditável por evidência;
- usuário consegue editar e restaurar versões;
- diagnóstico mostra gargalo e erro por etapa;
- código fica modular;
- runtime é instalador único e validado por benchmark;
- Groq continua disponível no auge dele;
- Cloudflare/Deepgram/local continuam como estratégia adaptativa;
- o projeto supera o Eskuta em motor, arquitetura, UX e distribuição.
