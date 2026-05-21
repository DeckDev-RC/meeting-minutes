# Protocolo pratico de benchmark gratuito

Este documento transforma a pesquisa em um caminho de medicao. A meta e separar fatos de achismo: velocidade real, qualidade da ata e pontos onde o algoritmo ainda perde informacao.

## Fontes gratuitas recomendadas

1. **MeetingBank**: melhor base para reunioes longas. A pagina oficial descreve 1.366 reunioes publicas, 3.579 horas de video, transcricoes, resumos de referencia, URLs de video e metadados. Use para medir velocidade em reunioes de 1 a 3 horas e qualidade de resumo.  
   Fonte: https://meetingbank.github.io/dataset/

2. **AMI Meeting Corpus**: melhor base para diarizacao e itens de ata. As anotacoes incluem transcricao ortografica, resumos abstrativos/extrativos e secoes como `DECISIONS`, `PROBLEMS/ISSUES` e `ACTIONS`. Use para calibrar falantes, decisoes e acoes.  
   Fonte: https://groups.inf.ed.ac.uk/ami/corpus/annotation.shtml

3. **PublicHearingBR**: melhor base gratuita em portugues brasileiro para texto longo. Tem 206 amostras de transcricoes de audiencias publicas com materia jornalistica e metadados estruturados. Use para avaliar sumarizacao e extracoes em PT-BR, mas nao como benchmark de audio/diarizacao.  
   Fonte: https://huggingface.co/datasets/unicamp-dl/PublicHearingBR/blob/main/README_PT.md

## Conjunto minimo

Para nao atrasar o produto, comece pequeno:

- 1 reuniao curta, 15 a 25 min, com 10 a 15 itens de gabarito.
- 1 reuniao longa, 2 a 3 h, com 20 a 40 itens de gabarito.
- 1 caso AMI com anotacoes de acoes/decisoes para validar falantes.
- 1 caso PublicHearingBR para validar robustez em portugues sem gastar com audio.

Depois que o fluxo estiver estavel, suba para 10 casos: 4 longos MeetingBank, 3 AMI, 2 PublicHearingBR e 1 reuniao real nossa com permissao de uso.

## Como montar o gabarito

Crie um manifesto JSON com uma entrada por reuniao. Cada item de gabarito deve ter termos obrigatorios, nao frases enormes. Isso evita que uma ata correta perca ponto por escrever a mesma ideia com outras palavras.

Exemplo:

```json
{
  "id": "action-001",
  "kind": "action",
  "text": "Ana deve enviar a proposta para validacao.",
  "requiredTerms": ["ana", "enviar", "proposta"]
}
```

Tipos aceitos: `decision`, `action`, `question`, `risk`, `topic`, `summary`.

## Como baixar amostras sem baixar tudo

MeetingBank em texto, split de teste:

```powershell
mkdir benchmarks\data -Force
curl.exe -L "https://datasets-server.huggingface.co/rows?dataset=huuuyeah/meetingbank&config=default&split=test&offset=0&length=20" -o benchmarks\data\meetingbank-test-20.json
```

PublicHearingBR em portugues:

```powershell
mkdir benchmarks\data -Force
curl.exe -L "https://datasets-server.huggingface.co/rows?dataset=unicamp-dl/PublicHearingBR&config=default&split=train&offset=0&length=20" -o benchmarks\data\publichearingbr-train-20.json
```

AMI deve ser baixado pelo portal oficial porque voce escolhe sinais/anotacoes por reuniao:

```text
https://groups.inf.ed.ac.uk/ami/download/
```

## Como rodar a avaliacao

Use o manifesto e um arquivo de execucao com a saida do app:

```powershell
npm run eval:meeting -- --manifest docs\evaluation\sample-benchmark-manifest.json --run docs\evaluation\sample-benchmark-run.json --format markdown
```

Formato JSON:

```powershell
npm run eval:meeting -- --manifest docs\evaluation\sample-benchmark-manifest.json --run docs\evaluation\sample-benchmark-run.json --format json
```

O comando sai com codigo `2` quando algum gate falha. Isso permite usar o benchmark em CI depois.

## Benchmark rapido de texto com Gemini

Para testar especificamente o gargalo de **extrair fatos + gerar ata** sem Groq e sem baixar audio, use o runner de texto. Ele baixa poucas linhas do Hugging Face ou usa um arquivo `rows.json`, divide a transcricao em blocos, chama o Gemini por bloco e gera:

- `text-benchmark-sources.json`
- `text-benchmark-manifest-draft.json`
- `text-benchmark-run.json`
- `text-benchmark-report.md`
- `*-minutes.html`

Dry-run, sem Gemini:

```powershell
npm run benchmark:text -- --dataset meetingbank --offset 0 --length 1 --max-cases 1 --dry-run
```

Execucao real com Gemini:

```powershell
$env:GEMINI_API_KEY="sua-chave"
npm run benchmark:text -- --dataset meetingbank --offset 0 --length 1 --max-cases 1 --transcript-char-limit 3000 --target-chars 3000 --concurrency 1
```

Para portugues:

```powershell
$env:GEMINI_API_KEY="sua-chave"
npm run benchmark:text -- --dataset publichearingbr --offset 0 --length 1 --max-cases 1 --transcript-char-limit 3000 --target-chars 3000 --concurrency 1
```

Esse benchmark mede velocidade das etapas de fatos/ata em texto. Ele nao substitui o benchmark de audio completo, porque nao mede extracao de audio, transcricao, diarizacao acustica nem DER.

No manifesto gerado automaticamente pelo runner, os campos de qualidade ficam sem gabarito manual; por isso o relatorio mostra `n/a` para precision/recall/F1. Para medir qualidade de verdade, preencha `referenceItems` manualmente com decisoes, acoes, perguntas, riscos ou topicos esperados.

## Metricas

- **Throughput**: segundos de audio divididos por segundos de processamento. `5.00x` significa processar 50 min em 10 min.
- **RTF**: real-time factor. Menor e melhor. `0.20` significa 20% do tempo do audio.
- **Precision**: quanto do que o app extraiu esta no gabarito.
- **Recall**: quanto do gabarito o app conseguiu capturar.
- **F1**: equilibrio entre precision e recall.
- **Speaker delta**: diferenca entre falantes esperados e detectados. Isso nao substitui DER; para DER precisamos de timestamps de falantes, principalmente do AMI.

Gates iniciais sugeridos:

```json
{
  "minThroughputX": 3,
  "maxRealTimeFactor": 0.35,
  "minFactRecall": 0.7,
  "minFactPrecision": 0.7
}
```

Esses valores sao ponto de partida, nao promessa de qualidade. O numero correto so aparece depois de rodar nas reunioes reais.

## Regra para otimizar sem achismo

Toda mudanca de algoritmo precisa gerar dois relatorios comparaveis:

1. **baseline**: o melhor resultado atual salvo em `benchmarks/runs/.../diarization-benchmark-report.json`.
2. **candidate**: a nova mudanca rodada no mesmo audio, mesmas transcricoes, mesmo numero esperado de falantes e mesma maquina.

Depois rode:

```powershell
npm run compare:diarization -- --baseline benchmarks\runs\diarization-modern-cpu-current\diarization-benchmark-report.json --candidate benchmarks\runs\nova-tentativa\diarization-benchmark-report.json --min-speedup 1.10 --max-rtf 0.09 --expected-speakers 4
```

Se o comando sair com codigo `2`, a mudanca nao entra como melhoria de performance. Ela pode ate ficar guardada como experimento, mas nao deve substituir o caminho do usuario.

Exemplo real que decidiu arquitetura:

```powershell
npm run compare:diarization -- --baseline benchmarks\runs\diarization-pyannote-current\diarization-benchmark-report.json --candidate benchmarks\runs\diarization-modern-cpu-current\diarization-benchmark-report.json --min-speedup 2 --max-rtf 0.2 --expected-speakers 4
```

Resultado: `modern-cpu` venceu pyannote por `10.261x` no wall-clock e manteve `4` falantes.

## Proximas melhorias praticas

1. Exportar automaticamente um `run.json` ao final do processamento do app.
2. Medir cada etapa separadamente: extrair audio, transcrever, diarizar, extrair fatos e gerar ata.
3. Adicionar DER para AMI quando tivermos parse das anotacoes temporais de falantes.
4. Salvar o hardware usado no benchmark: CPU, RAM, modelo de GPU, Windows e versao do app.

## Benchmark de diarizacao CPU-only moderna

O gargalo medido no AMI `ES2002a` foi a diarizacao. Alem do backend local `sherpa-onnx`, testamos o pacote Python `diarize==0.1.2`, CPU-only, Apache-2.0, sem token Hugging Face e sem GPU. Ele usa Silero VAD + WeSpeaker/ONNX e baixa um modelo WeSpeaker pequeno na primeira execucao.

Setup isolado:

```powershell
npm run setup:diarize-cpu
```

Run com numero conhecido de falantes:

```powershell
npm run benchmark:diarize-cpu -- --audio benchmarks\runs\ami-es2002a-e2e\normalized-audio.wav --out-dir benchmarks\runs\diarize-cpu-ami-num4-warm --num-speakers 4
```

Resultados reais no AMI `ES2002a`, audio de `1272.64s`, CPU Intel i5-13500T, sem GPU NVIDIA:

| Backend | Configuracao | Tempo | RTF | Velocidade | Projecao 3h |
| --- | ---: | ---: | ---: | ---: | ---: |
| sherpa-onnx baseline | full audio, 2 threads | 324.11s | 0.255 | 3.93x | 45.8 min |
| sherpa-onnx hybrid | chunks WAV, 8 threads | 197.78s | 0.155 | 6.43x | 28.0 min |
| diarize CPU | warm cache, `num_speakers=4` | 98.23s | 0.077 | 12.96x | 13.9 min |
| app `modern-cpu` | Rust runner, `num_speakers=4` | 103.50s | 0.081 | 12.30x | 14.6 min |
| app `modern-cpu` | rodada atual, `num_speakers=4` | 120.25s | 0.094 | 10.58x | 17.0 min |
| app `modern-cpu-chunked` | normalizado por `expected_speakers=4`, 2 chunks em paralelo | 85.12s | 0.067 | 14.95x | 12.0 min |
| app `modern-cpu` | backend customizado reutilizando WeSpeaker, `num_speakers=4` | 65.83s | 0.052 | 19.33x | 9.3 min |
| app `modern-cpu-chunked` | batch corrigido + centroid stitch, `expected_speakers=4` | 78.18s | 0.061 | 16.28x | 11.1 min |
| pyannote Community-1 | CPU Windows, `num_speakers=4` | 1233.89s | 0.970 | 1.03x | 174.5 min |
| diarize CPU | auto speaker count | 116.00s | 0.091 | 10.97x | 16.4 min |

Observacoes:

- O primeiro run do `diarize` inclui download do modelo WeSpeaker; use o segundo run para medir operacao real.
- Com `num_speakers=4`, o AMI detectou 4 falantes. Sem esse parametro, detectou 3 falantes neste caso.
- O modo `auto` do app tenta usar o backend `modern-cpu` quando `.venv-diarize` e `scripts/diarize_cpu_backend.py` existem; se nao houver backend, cai para Sherpa/local. O modo explicito `modern-cpu` falha com erro claro se o backend nao estiver instalado.
- Esses numeros medem velocidade e contagem de falantes/segmentos. Precisao real ainda precisa de DER com anotacoes temporais do AMI.
- `modern-cpu-chunked` primeiro foi mais rapido, mas ficou com `12` falantes no AMI `ES2002a` contra `4` esperados. Depois da normalizacao por `expected_speakers`, passou no gate: `85.12s`, `14.95x`, `4` falantes e `102` segmentos. O app usa esse caminho apenas quando ha numero esperado de falantes; caso contrario ou se falhar, cai para `modern-cpu` inteiro.
- A rodada de 2026-05-21 corrigiu o bug do resumo batch, adicionou matching por centroide e trocou o motor Python para reutilizar a instancia `wespeakerruntime.Speaker`. Com isso, o `modern-cpu` inteiro ficou mais rapido que o chunked no AMI `ES2002a`; o modo `auto` deve preferir `modern-cpu` e deixar `modern-cpu-chunked` para modo explicito/perfil de precisao.
- O setup CPU fixa `torch==2.8.0` e `torchaudio==2.8.0` em `scripts/requirements-diarize-cpu.txt`. O backend registra aviso em relatorio se detectar `torchaudio>=2.9`, porque `silero-vad` ainda usa `torchaudio.sox_effects`.
- Pyannote Community-1 foi validado com token Hugging Face liberado, mas no CPU Windows desta maquina ficou praticamente em tempo real: `1233.89s` para `1272.64s` de audio. Isso e `10.26x` mais lento que o `modern-cpu` nesta mesma amostra. Portanto, pyannote deve ficar como backend experimental/benchmark explicito, nao como padrao do modo Precisao.

Benchmark integrado via Rust:

```powershell
cargo run --bin diarization_benchmark -- --mode modern-cpu --expected-speakers 4 --audio ..\benchmarks\runs\ami-es2002a-e2e\normalized-audio.wav --chunks ..\benchmarks\runs\ami-es2002a-e2e-wav-chunks\chunks.json --segments ..\benchmarks\runs\ami-es2002a-e2e\transcription-segments.json --out-dir ..\benchmarks\runs\diarization-modern-cpu-num4-rust
```

## Benchmark E2E adaptativo

Depois da integracao do motor adaptativo, o runner E2E passou a iniciar a diarizacao local assim que o WAV normalizado existe e a extrair fatos em paralelo enquanto aguarda o alinhamento final de falantes.

Run real no AMI `ES2002a`, audio de `1272.64s`, CPU Intel i5-13500T, usando Groq/Gemini configurados localmente:

```powershell
cargo run --bin e2e_benchmark -- --input ..\benchmarks\data\ami\ES2002a.Mix-Headset.wav --id ami-es2002a-adaptive --out-dir ..\benchmarks\runs\ami-es2002a-adaptive --source "AMI ES2002a Mix-Headset adaptive" --expected-speakers 4 --diarization-mode auto --transcribe-concurrency 3 --facts-concurrency 2
```

Resultado:

| Pipeline | Tempo E2E | RTF | Velocidade | Falantes | Observacao |
| --- | ---: | ---: | ---: | ---: | --- |
| baseline anterior | 387.90s | 0.305 | 3.28x | n/a | fluxo linear medido antes |
| adaptativo | 118.45s | 0.093 | 10.74x | 4 | diarizacao e fatos sobrepostos |
| adaptativo + backend reutilizado | 85.57s | 0.067 | 14.87x | 4 | `modern-cpu`, WeSpeaker reutilizado |
| adaptativo + segunda passada moderna | 150.85s | 0.119 | 8.44x | 4 | 1 subjanela suspeita reprocessada |
| adaptativo + Sherpa seletivo | 445.82s | 0.350 | 2.85x | 4 | rejeitado: custo alto mesmo em 1 chunk |

Tempos do run adaptativo:

| Etapa | Tempo |
| --- | ---: |
| extract_audio | 0.54s |
| create_smart_chunks | 0.72s |
| transcribe | 7.81s |
| diarize_speculative | 99.48s |
| extract_facts_parallel | 91.67s |
| generate_minutes | 17.71s |

Os tempos `diarize_speculative` e `extract_facts_parallel` se sobrepoem; por isso a soma das etapas e maior que o wall-clock total.

Run `ami-es2002a-adaptive-centroid-reuse` de 2026-05-21:

| Etapa | Tempo |
| --- | ---: |
| extract_audio | 1.19s |
| create_smart_chunks | 0.74s |
| transcribe | 4.97s |
| diarize_speculative | 66.82s |
| extract_facts_parallel | 61.84s |
| generate_minutes | 16.82s |

## Benchmark E2E reuniao real `2026-05-08 15-48-29`

Arquivo:

```text
G:\Drives compartilhados\Drive Dev's\Anotações_Equipe\Marcos Paulo\Reunião\2026-05-08 15-48-29.mp4
```

O run `d34b0ff3-9a83-466a-a71f-203b96fbbd7c` salvo pelo app em 2026-05-21 levou `856.70s` para `992.93s` de audio. A investigacao no SQLite local mostrou que a reuniao foi criada em `2026-05-21T20:50:27Z`, mas os chunks so foram persistidos em `2026-05-21T21:01:19Z`; portanto o gargalo estava antes da transcricao, na preparacao de audio/chunks. A causa pratica era escrever `_audio.wav`, cache de silencio e `_chunks` diretamente no Google Drive compartilhado.

Depois de medir o mesmo arquivo com intermediarios locais:

```powershell
cargo run --manifest-path src-tauri\Cargo.toml --bin e2e_benchmark -- --input "G:\Drives compartilhados\Drive Dev's\Anotações_Equipe\Marcos Paulo\Reunião\2026-05-08 15-48-29.mp4" --id real-2026-05-08-154829-current --out-dir "c:\C\pop\meeting-minutes\benchmarks\runs\real-2026-05-08-154829-current" --source "Reunião Marcos Paulo 2026-05-08 15-48-29" --expected-speakers 2 --diarization-mode auto --transcribe-concurrency 3 --facts-concurrency 2
```

Resultado:

| Pipeline | Tempo E2E | RTF | Velocidade | Falantes | Observacao |
| --- | ---: | ---: | ---: | ---: | --- |
| app antigo em Drive compartilhado | 856.70s | 0.863 | 1.16x | 2 | gargalo na preparacao de audio/chunks em `G:\` |
| runner E2E com intermediarios locais | 94.17s | 0.095 | 10.48x | 2 | `modern-cpu`, 3 chunks |
| runner E2E com concorrencia balanced | 97.12s | 0.098 | 10.16x | 2 | transcricao 4, fatos 3 |

Tempos do run local `real-2026-05-08-154829-current`:

| Etapa | Tempo |
| --- | ---: |
| extract_audio | 1.60s |
| create_smart_chunks | 0.64s |
| transcribe | 4.05s |
| diarize_speculative | 73.23s |
| extract_facts_parallel | 69.18s |
| generate_minutes | 18.70s |

Conclusao: para essa reuniao, a otimizacao de maior impacto e manter WAV/chunks/cache em workspace local e salvar no Drive apenas os artefatos finais. Depois disso, o gargalo real passa a ser `max(diarize_speculative, extract_facts_parallel)` mais a geracao final.

Nova investigacao no mesmo dia, com o arquivo copiado para `C:\2026-05-08 15-48-29.mp4`, mostrou outro gargalo no app instalado: o caminho padrao ainda exportava chunks a partir do MP4 original. O FFmpeg observado em producao rodava comandos como `-ss ... -i C:\2026-05-08 15-48-29.mp4 -t ... -ar 16000 -ac 1 -c:a flac ...`, sem `-map 0:a:0` e sem `-vn`, mantendo tres processos em CPU por varios minutos e gerando chunks de `0` bytes durante a espera. A correcao foi tornar `singlePassSilence` o padrao de `prepare_audio_and_chunks`, exportando chunks a partir do WAV normalizado, e adicionar `-map 0:a:0 -vn` ao fallback paralelo.

Benchmark isolado de preparacao:

| Arquivo | Duracao | FPS | single-pass WAV/chunks | paralelo a partir do MP4 | Observacao |
| --- | ---: | ---: | ---: | ---: | --- |
| `C:\2026-05-08 15-48-29.mp4` | 986.93s | 60 | 1.12s | 58.64s | caso lento reportado |
| `C:\Users\User\Videos\2026-05-15 10-05-59.mp4` | 1218.23s | 30 | 1.59s | 39.88s | reuniao mais longa, mas menos custosa |

Conclusao pratica:

- A segunda passada seletiva nao deve usar Sherpa no caminho rapido: no AMI `ES2002a`, mesmo uma tentativa seletiva custou `445.82s` E2E.
- O caminho viavel gratuito e reprocessar apenas sub-janelas curtas com o backend moderno CPU. No run medido, isso ficou em `150.85s`, ainda `2.57x` mais rapido que o baseline linear de `387.90s`.
- Para manter o wow de velocidade, o app limita o refinamento a uma janela suspeita no fluxo padrao.
