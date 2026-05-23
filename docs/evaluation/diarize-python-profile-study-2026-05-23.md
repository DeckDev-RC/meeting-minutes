# Diarize Python backend profile study - 2026-05-23

Objetivo: buscar ganho no backend Python atual, sem trocar runtime/modelo e sem passar novamente por transcricao, LLM ou APIs.

## Amostra

- Reuniao real de aproximadamente 3h03.
- Chunks ja exportados em FLAC a partir do run `benchmarks/runs/diarization-runtime-20260523-094744`.
- Estudo salvo em `benchmarks/runs/diarize-python-profile-20260523-105743`.
- A soma de audio dos chunks inclui overlap entre blocos, por isso pode ser maior que a duracao real da reuniao.

## Ambiente

- Python: 3.11.9 em `.venv-diarize`.
- `torch`: 2.8.0.
- `torchaudio`: 2.8.0.
- `diarize`: 0.1.2.
- `silero-vad`: 6.2.1.
- `wespeakerruntime`: 1.0.1.
- Sem warnings de dependencia no backend local. O risco conhecido continua sendo `torchaudio>=2.9`.

## Resultado curto - 1 chunk / 6 min

| Caso | Wall | Speed | Falantes | Segmentos | Embeddings | VAD | Embeddings | Clustering |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| quality | 38.95s | 9.24x | 3 | 73 | 416 | 5.62s | 18.55s | 12.64s |
| balanced | 26.13s | 13.78x | 4 | 82 | 255 | 3.98s | 12.18s | 8.80s |
| fast | 23.43s | 15.37x | 3 | 70 | 227 | 3.87s | 10.17s | 8.21s |
| boundary | 26.76s | 13.45x | 4 | 80 | 281 | 4.04s | 11.29s | 8.96s |
| balanced + max 8 | 19.94s | 18.05x | 4 | 82 | 255 | 3.84s | 11.56s | 3.43s |
| balanced + max 12 | 21.83s | 16.49x | 4 | 82 | 255 | 4.34s | 11.44s | 4.74s |
| balanced + num 4 | 18.30s | 19.67x | 4 | 82 | 255 | 4.14s | 11.56s | 1.45s |

Leitura: reduzir a densidade para `fast` muda a estrutura dos falantes/segmentos. O ganho seguro veio de limitar a busca do clustering, nao de reduzir janela de embedding.

## Resultado medio - 5 chunks paralelos

| Caso | Wall | Speed | Falantes locais | Segmentos locais | Embeddings | Soma VAD | Soma embeddings | Soma clustering |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| balanced | 52.37s | 34.37x | 4 | 365 | 1239 | 30.47s | 80.02s | 130.99s |
| balanced + max 4 | 25.20s | 71.44x | 4 | 365 | 1239 | 29.26s | 75.49s | 9.20s |
| balanced + min 2/max 4 | 26.82s | 67.12x | 4 | 365 | 1239 | 30.75s | 81.76s | 7.32s |
| balanced + max 5 | 27.90s | 64.51x | 4 | 365 | 1239 | 28.73s | 85.13s | 10.54s |
| balanced + max 8 | 31.82s | 56.57x | 4 | 365 | 1239 | 32.62s | 86.57s | 25.50s |
| balanced + num 4 | 26.59s | 67.70x | 4 | 543 | 1239 | 30.57s | 84.06s | 3.29s |
| fast + max 8 | 30.09s | 59.82x | 3 | 359 | 1121 | 27.97s | 80.71s | 29.86s |

Leitura: `--num-speakers 4` e rapido, mas forcou quatro falantes em cada chunk e aumentou os segmentos locais de 365 para 543. Para chunked, o numero esperado deve ser usado como teto/cap de busca, nao como obrigacao exata por chunk.

## Validacao de integracao Rust - 5 chunks

Depois da mudanca no comando Tauri/Rust, o binario `diarization_benchmark` foi executado em `modern-cpu-chunked` com os mesmos 5 chunks.

| Caso | Wall | Speed | Segmentos finais | Falantes finais |
| --- | ---: | ---: | ---: | ---: |
| Rust modern-cpu-chunked + max 8 | 36.84s | 48.53x | 111 | 4 |

O tempo inclui o fluxo Rust, chamada ao Python, leitura dos resultados e stitching/alinhamento final, portanto fica acima do backend Python direto, mas confirma que a nova regra esta ativa no caminho usado pelo app.

## Resultado completo - chunks da reuniao de 3h

| Caso | Wall | Speed | Observacao |
| --- | ---: | ---: | --- |
| modern-cpu-chunked anterior | 307.30s | 35.72x | Benchmark Rust anterior, sem limite de clustering por chunk |
| balanced + max 8 | 165.59s | 66.84x | Backend Python direto, sem transcricao/LLM |

Ganho medido: aproximadamente 46% menos wall time no backend Python de diarizacao para esta reuniao.

## Decisao aplicada

- Manter `balanced` como perfil padrao, porque `fast` mudou contagem de falantes em amostras reais.
- Em batch/chunked, passar `--max-speakers` ao backend Python:
  - sem numero esperado: teto automatico 8;
  - com numero esperado: usar o numero esperado como teto, limitado a 12.
- Nao usar `--num-speakers` por chunk no modo chunked, porque pode superfragmentar chunks que nao contem todos os falantes da reuniao.
- Manter `--num-speakers` apenas no modo full-audio curto, onde o audio inteiro representa a reuniao completa.

## Proximos estudos

- Medir uma reuniao com mais de 8 falantes reais antes de reduzir o teto automatico abaixo de 8.
- Investigar daemon/processo persistente para manter modelos carregados entre execucoes, porque hoje a reutilizacao ja existe por worker durante uma execucao, mas nao entre reunioes.
- Medir VAD e embeddings com cache por chunk para retomadas interrompidas.
