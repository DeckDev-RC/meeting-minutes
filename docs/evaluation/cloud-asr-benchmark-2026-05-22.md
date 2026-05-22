# Benchmark de ASR em nuvem - 2026-05-22

Objetivo: comparar Cloudflare Workers AI, Deepgram Nova-3 e xAI Speech to Text como alternativas ao Groq para transcricao de reunioes longas, sem integrar nenhum provedor novo ao app antes de medir custo, velocidade e qualidade.

## Provedores

| Provedor | Modelo/API | Preco publico usado no estimador | Observacoes para o nosso caso |
| --- | --- | ---: | --- |
| Cloudflare | `@cf/openai/whisper-large-v3-turbo` | US$ 0.00051/min | Mais barato da matriz; usa audio em base64 no endpoint Workers AI. |
| Deepgram | `nova-3` pre-recorded | US$ 0.005833/min + US$ 0.0013/min com `keyterm` | Melhor candidato de qualidade/recursos; suporta `keyterm` no Nova-3 e portugues `pt`, `pt-BR`, `pt-PT`. |
| xAI | `/v1/stt` REST | US$ 0.001667/min | Meio-termo de preco; suporta `keyterm`, timestamps por palavra e limite de arquivo maior. |

Fontes consultadas em 2026-05-22:
- Cloudflare Workers AI pricing: https://developers.cloudflare.com/workers-ai/platform/pricing/
- Cloudflare Whisper large-v3-turbo model: https://developers.cloudflare.com/workers-ai/models/whisper-large-v3-turbo/
- Deepgram pricing: https://deepgram.com/pricing
- Deepgram pre-recorded API: https://developers.deepgram.com/reference/speech-to-text/listen-pre-recorded
- Deepgram Nova-3 portugues: https://deepgram.com/learn/deepgram-expands-nova-3-with-spanish-french-and-portuguese-support
- xAI Speech to Text model/pricing: https://docs.x.ai/developers/models/speech-to-text
- xAI Speech to Text guide: https://docs.x.ai/developers/model-capabilities/audio/speech-to-text

## Arquivos adicionados

- `scripts/cloud_asr_benchmark.py`: harness isolado para Cloudflare, Deepgram e xAI.
- `scripts/cloud_asr_benchmark_test.py`: testes unitarios de parsing, custo estimado e CLI.
- `package.json`: script `benchmark:cloud-asr`.

## Credenciais

O script nao imprime segredos. Ele so verifica se as variaveis existem.

No Windows, o harness tambem consulta as variaveis persistidas nos escopos `User` e `Machine` quando elas ainda nao foram herdadas pelo processo atual.

```powershell
$env:CLOUDFLARE_ACCOUNT_ID="..."
$env:CLOUDFLARE_API_TOKEN="..."
$env:DEEPGRAM_API_KEY="..."
$env:XAI_API_KEY="..."
```

Pre-checagem:

```powershell
python scripts\cloud_asr_benchmark.py --list-auth
```

Status desta maquina no momento da validacao:

```json
{
  "cloudflare": true,
  "deepgram": true,
  "xai": true
}
```

## Comando de benchmark

Reuniao problematica ja preparada em chunks pelo benchmark Parakeet:

```powershell
npm run benchmark:cloud-asr -- --chunks-json benchmarks\runs\e2e-parakeet-20260522-2026-05-08\chunks.json --out-dir benchmarks\runs\cloud-asr-20260522-2026-05-08 --language pt --concurrency 3
```

O `--concurrency 3` transcreve os tres chunks em paralelo dentro de cada provedor, para aproximar o comportamento do pipeline atual do app.

Por padrao, a matriz ativa roda apenas Cloudflare e Deepgram. xAI fica fora do padrao ate a conta/time ter credito/licenca, mas ainda pode ser chamado manualmente com `--provider xai`.

O `chunks.json` usado contem 3 blocos FLAC:

| Chunk | Duracao |
| --- | ---: |
| `chunk_000.flac` | 361.41s |
| `chunk_001.flac` | 358.86s |
| `chunk_002.flac` | 272.63s |

Duracao real aproximada: 986.90s, ou 16m27s. Duracao cobrada no benchmark com sobreposicao de chunks: 992.90s.

## Status

Cloudflare e Deepgram ja foram executados na amostra de 3 chunks. xAI esta configurado, mas a API bloqueou a execucao por falta de credito/licenca no time.

| Provedor | Tempo | Velocidade | Custo estimado | Segmentos | Caracteres |
| --- | ---: | ---: | ---: | ---: | ---: |
| Cloudflare | 20.245s | 49.04x | US$ 0.00844 | 350 | 13070 |
| Deepgram | 17.427s | 56.98x | US$ 0.118045 com `keyterm` | 233 | 13888 |
| xAI | bloqueado | bloqueado | n/a | n/a | n/a |

Contagem simples de nomes/termos na transcricao:

| Provedor | Caio | Thay | Manuela | Emanuela | Renato | Rafaela | WhatsApp | Drive |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Cloudflare | 5 | 0 | 1 | 3 | 7 | 2 | 7 | 6 |
| Deepgram | 6 | 0 | 4 | 0 | 8 | 3 | 9 | 9 |

xAI:

- tentativa com os 3 chunks: `502 Bad Gateway` vindo do gateway da xAI;
- probe isolado de 30s em WAV: `403`, sem credito/licenca no time;
- probe isolado de 30s em FLAC: `403`, sem credito/licenca no time.

Conclusao parcial: a chave esta valida o suficiente para autenticar, mas a conta/time da xAI ainda nao pode executar STT. Nao ha dado de velocidade/qualidade ate adicionar credito/licenca no console xAI.

Arquivos gerados:

- `benchmarks/runs/cloud-asr-20260522-2026-05-08/summary.json`
- `benchmarks/runs/cloud-asr-20260522-2026-05-08/<provider>/report.json`
- `benchmarks/runs/cloud-asr-20260522-2026-05-08/<provider>/transcription-segments.json`

## Criterios de decisao

Medir para cada provedor:

- tempo total e velocidade em relacao ao tempo real;
- custo estimado por reuniao curta e por reuniao de 4h46;
- nomes proprios: principalmente `Caio`, `Manuela`, `Renato`, `Rafaela`;
- termos tecnicos: `leitor de documentos`, `WhatsApp`, `Drive`;
- repeticao/alucinacao;
- impacto na ata final depois de passar pelo pipeline local existente.

Recomendacao operacional: rodar primeiro na amostra de 16m27s. Se um provedor errar nomes ou repetir texto nessa amostra, ele nao deve virar backend padrao antes de teste maior.

## E2E ate ata - app pipeline

Runs executados depois da integracao dos comandos Tauri:

| Run | Transcricao | Total E2E | Velocidade E2E | Etapa transcrever | Decisoes | Acoes | Observacao |
| --- | --- | ---: | ---: | ---: | ---: | ---: | --- |
| `e2e-cloudflare-adaptive-20260522` | Cloudflare | 72.32s | 13.65x | 22.15s | 4 | 19 | Melhor custo; ainda mostrou `Emanuela` em contagem simples. |
| `e2e-deepgram-adaptive-20260522` | Deepgram | 73.54s | 13.42x | 3.87s | 4 | 15 | Melhor nomes; custo maior. |

Arquivos:

- `benchmarks/runs/e2e-cloudflare-adaptive-20260522/minutes.html`
- `benchmarks/runs/e2e-deepgram-adaptive-20260522/minutes.html`
- `benchmarks/runs/e2e-cloudflare-adaptive-20260522/benchmark-report.json`
- `benchmarks/runs/e2e-deepgram-adaptive-20260522/benchmark-report.json`

Leitura: depois que transcricao ficou rapida, o tempo total ficou dominado por diarizacao especulativa e extracao de fatos. Portanto a estrategia correta e manter Cloudflare como base economica e usar Deepgram seletivamente nos chunks suspeitos, nao trocar tudo para Deepgram.
