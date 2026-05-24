# Sidecar Runtime Benchmark - 2026-05-24

## Contexto

Objetivo: medir o sidecar experimental, que era candidato da Fase 4, contra o runtime atual antes de qualquer troca no pipeline instalado.

O runtime atual usa ambientes separados:

- `.venv-diarize`: `diarize`, `torch`, `torchaudio`, `silero-vad`, `wespeakerruntime`.
- `.venv-transcribe`: `faster-whisper`, `ctranslate2`.

Por isso, a matriz validou dois perfis PyInstaller separados (`diarize` e `transcribe`). Depois das medições, o sidecar deixou de ser candidato de produto e não há plano para criar um perfil `full`.

Artefatos brutos: `benchmarks/runs/sidecar-runtime-20260524`.

## Ajuste Necessario No Build

O primeiro build PyInstaller subiu apenas o comando `health`, mas nao empacotou os imports dinamicos usados por `meeting_minutes_sidecar.py`.

Falhas encontradas:

- imports dinamicos nao vistos pelo PyInstaller;
- metadata de pacote ausente no `health`;
- `numpy._core._exceptions` ausente;
- `scipy._cyutility` ausente;
- `silero_vad/data/silero_vad.jit` ausente.

O script `scripts/build_meeting_minutes_sidecar.ps1` foi ajustado com perfis (`minimal`, `diarize`, `transcribe`, `full`), hidden imports, metadata, submodules e dados de pacote.

## Diarizacao

Amostra: `benchmarks/runs/diarization-runtime-20260523-094744/chunks-wav-1chunk.json`.

Audio processado: 360s.

| Modo | Build | Tamanho | Startup health | Wall interno | Wall externo | Speed | Resultado |
|---|---:|---:|---:|---:|---:|---:|---|
| Backend atual direto | n/a | n/a | n/a | 25.54s | 26.06s | 14.10x | ok |
| Sidecar Python no venv | n/a | n/a | n/a | 25.01s | 25.50s | 14.39x | ok |
| Sidecar PyInstaller `onedir` | ~400s build | 580.5 MB | ~0.28-0.33s quente | 35.49s | 36.12s | 10.15x | ok |
| Sidecar PyInstaller `onefile` | ~440s build | 220.1 MB | ~6.0s quente | 25.74s | 32.26s | 13.99x | ok |

Observacoes:

- O wrapper Python puro esta em paridade com o backend atual.
- `onedir` ficou funcional, mas perdeu desempenho real nessa amostra.
- `onefile` manteve o tempo interno parecido, mas adicionou cerca de 6.5s de custo externo por chamada por causa da extracao/startup.
- O `commandWallClockSec` dentro do JSON nao captura todo o custo de startup do processo PyInstaller; o tempo externo do PowerShell e o numero mais correto para UX.

## Transcricao Local

Amostra: `benchmarks/runs/cloud-asr-20260522-2026-05-08/xai-probes/probe_30s.flac`.

Modelo: faster-whisper `turbo`, CPU `int8`, `cpuThreads=4`.

| Modo | Build | Tamanho | Startup health | Wall interno | Wall externo | Segmentos | Resultado |
|---|---:|---:|---:|---:|---:|---:|---|
| Backend atual direto | n/a | n/a | n/a | 14.12s | 14.33s | 1 | ok |
| Sidecar Python no venv | n/a | n/a | n/a | 13.85s | 14.11s | 1 | ok |
| Sidecar PyInstaller `onedir` | ~49s build | 246.5 MB | ~0.19-0.22s quente | 19.75s | 20.09s | 1 | ok |
| Sidecar PyInstaller `onefile` | ~66s build | 96.0 MB | ~2.0s quente | 14.99s | 17.07s | 1 | ok |

Observacoes:

- O texto produzido foi equivalente na amostra.
- `onedir` ficou mais lento que o runtime atual.
- `onefile` tambem perdeu no tempo externo por causa da extracao/startup.

## Conclusao

O sidecar PyInstaller ainda nao deve substituir o runtime atual.

Decisao apos a matriz inicial:

1. Manter o runtime atual como padrao.
2. Nao usar sidecar PyInstaller CLI como runtime de produto.
3. Nao usar `onefile` para chamadas frequentes do pipeline; o custo de startup aparece diretamente na UX.
4. A unica hipotese ainda nao eliminada naquele momento era processo persistente; ela foi medida depois e tambem nao superou o runtime atual.

O caminho correto para Fase 4 nao e trocar para PyInstaller CLI por chamada. A decisao final depois da probe persistente e manter o runtime atual empacotado no instalador.

## Probe Persistente

Depois do benchmark de CLI por chamada, foi criado um prototipo experimental `serve` por JSONL.

Comando base:

```powershell
python scripts\meeting_minutes_sidecar.py serve
```

Artefatos brutos: `benchmarks/runs/sidecar-persistent-20260524`.

### Transcricao Local Persistente

Amostra: `benchmarks/runs/cloud-asr-20260522-2026-05-08/xai-probes/probe_30s.flac`.

Ambiente: `.venv-transcribe`, faster-whisper `turbo`, CPU `int8`, `cpuThreads=4`.

Wall externo total do processo: 25.57s para `t1+t2+shutdown`.

| Pedido | Wall interno | Cache | Segmentos | Resultado |
|---|---:|---|---:|---|
| `t1` | 15.61s | `engineCacheHit=false` | 1 | ok |
| `t2` | 9.70s | `engineCacheHit=true` | 1 | ok |

Leitura:

- O processo persistente evitou recriar o engine/modelo no segundo pedido.
- Na amostra curta, o segundo pedido caiu de ~14-15s para ~9.7s.
- Isso confirmou que valia testar sidecar persistente antes de fechar a decisao, mas ainda nao justificava trocar o runtime.

### Pool Persistente De Diarizacao

Depois da probe inicial, o prototipo `serve` ganhou um pool persistente para `numThreads > 1`.

Comportamento atual:

- `numThreads <= 1`: reutiliza uma unica funcao/modelo de diarizacao;
- `numThreads > 1`: reutiliza um pool de workers por quantidade normalizada de workers;
- cada worker carrega sua propria funcao/modelo uma vez;
- se a quantidade de workers muda, o pool antigo e encerrado e outro e criado.

Isto nao troca o pipeline instalado. O benchmark real abaixo fechou a decisao: o `serve` persistente com `numThreads > 1` nao deve substituir o runtime atual.

## Benchmark Real Do Pool Persistente

Artefatos brutos: `benchmarks/runs/sidecar-persistent-diarization-20260524`.

Amostra disponivel: reuniao real de aproximadamente 3h03, 31 chunks WAV, `numThreads=6`, `embeddingProfile=balanced`.

Nao foi encontrado artefato local reaproveitavel da reuniao de 4h46. O banco instalado em `%APPDATA%/com.agregar.meeting-minutes/db.sqlite` tinha tres processamentos salvos, todos com 31 chunks e `10977.43s` de audio. O benchmark de 4h46 fica bloqueado ate termos novamente os chunks/audio dessa reuniao.

| Modo | Cache | Wall comando | Wall interno | Speed | Chunks | Falantes | Segmentos | Resultado |
|---|---:|---:|---:|---:|---:|---:|---:|---|
| Backend atual direto | n/a | n/a | 305.49s | 36.23x | 31 | 5 | 2157 | ok |
| Sidecar persistente cold | miss | 318.71s | 317.40s | 34.87x | 31 | 5 | 2157 | ok |
| Sidecar persistente warm | hit | 312.87s | 312.74s | 35.39x | 31 | 5 | 2157 | ok |

Validacao de equivalencia:

- `chunkCount`, `speakerCount` e `segmentCount` bateram nos tres modos;
- a estrutura normalizada por chunk (`index`, lista de falantes e quantidade de segmentos) gerou o mesmo hash nos tres modos: `9866f11f43bb16454247de7c3d6eb24f6d9f8f9d76119db87103f23b8d4012da`;
- portanto nao houve regressao observada na saida do backend de diarizacao.

Leitura:

- O pool persistente funcionou: a segunda chamada veio com `diarizePoolCacheHit=true`.
- Na probe curta de 2 chunks, o warm pool melhorou a segunda chamada porque o custo de carga do modelo pesa mais.
- No caso real de 31 chunks, o warm pool nao superou o backend atual. A diferenca ficou dentro/contra o sidecar: `312.74s` contra `305.49s`.
- A raiz pratica e que o tempo full e dominado por VAD, embeddings e clustering por chunk. Economizar carga do modelo por worker nao muda suficientemente o custo total quando cada worker processa varios chunks longos.

Decisao final:

1. Nao trocar o runtime instalado para sidecar persistente agora.
2. Remover sidecar da Fase 4 como caminho de produto.
3. O proximo ganho real deve atacar o custo por chunk no backend Python atual: densidade de embeddings, VAD/cache de audio, perfil por duracao e matriz `numThreads`/chunks.
4. Manter scripts/artefatos de sidecar apenas para reproducao historica dos benchmarks, sem integrar no Tauri.
