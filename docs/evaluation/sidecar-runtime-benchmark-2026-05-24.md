# Sidecar Runtime Benchmark - 2026-05-24

## Contexto

Objetivo: medir o sidecar experimental da Fase 4 contra o runtime atual antes de qualquer troca no pipeline instalado.

O runtime atual usa ambientes separados:

- `.venv-diarize`: `diarize`, `torch`, `torchaudio`, `silero-vad`, `wespeakerruntime`.
- `.venv-transcribe`: `faster-whisper`, `ctranslate2`.

Por isso, a matriz validou dois perfis PyInstaller separados (`diarize` e `transcribe`). Um sidecar unico `full` ainda exige ambiente combinado antes de virar candidato final.

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

Decisao recomendada:

1. Manter o runtime atual como padrao.
2. Manter o sidecar como experimental/benchmark.
3. Nao usar `onefile` para chamadas frequentes do pipeline; o custo de startup aparece diretamente na UX.
4. Se insistirmos em sidecar, preferir `onedir` ou um processo persistente, nao CLI nova por chunk/etapa.
5. Antes de sidecar unico, criar um ambiente combinado e testar `Profile=full`; ele provavelmente sera maior que os perfis separados e precisa provar paridade.

O caminho mais promissor para Fase 4 nao e trocar para PyInstaller CLI por chamada. E manter o runtime atual ou criar um sidecar persistente que carrega o modelo uma vez e aceita multiplas requisicoes.

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
- Isso confirma que a direcao correta e sidecar persistente, nao PyInstaller CLI por chamada.

### Pool Persistente De Diarizacao

Depois da probe inicial, o prototipo `serve` ganhou um pool persistente para `numThreads > 1`.

Comportamento atual:

- `numThreads <= 1`: reutiliza uma unica funcao/modelo de diarizacao;
- `numThreads > 1`: reutiliza um pool de workers por quantidade normalizada de workers;
- cada worker carrega sua propria funcao/modelo uma vez;
- se a quantidade de workers muda, o pool antigo e encerrado e outro e criado.

Isto ainda nao troca o pipeline instalado. O proximo benchmark real deve medir o `serve` persistente com `numThreads > 1` contra o runtime atual usando as reunioes de 3h e 4h46, olhando tempo da etapa 3, estabilidade dos falantes e equivalencia da ata.
