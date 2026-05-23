# Benchmark de Runtime de Diarizacao - 2026-05-23

## Caso medido

- Reuniao: `b4b857ff-dbd5-439e-af50-c611522a7909`
- Origem: reuniao real de aproximadamente 3h03
- Duracao de audio medida: `10977.43s`
- Chunks: `31`
- Segmentos de transcricao cacheados: `2280`
- Transcricao/IA: nao executadas; o benchmark reaproveitou os chunks e segmentos ja salvos.
- Artefatos locais: `benchmarks/runs/diarization-runtime-20260523-094744`

## Resultados

| Runtime | Escopo | Tempo | Velocidade | Falantes | Observacao |
| --- | ---: | ---: | ---: | ---: | --- |
| modern-cpu-chunked | 31 chunks / 3h03 | `307.30s` | `35.72x` | `9` | Baseline atual aprovado |
| ONNX/sherpa CPU | 31 chunks / 3h03 | timeout `>900s` | abaixo do baseline | n/a | Reprovado para uso real |
| ONNX/sherpa CPU, threads=2 | 31 chunks / 3h03 | timeout `>900s` | abaixo do baseline | n/a | Reprovado mesmo com mais paralelismo |
| modern-cpu-chunked | 1 chunk / 6min | `27.46s` | `13.11x` | `4` | Comparacao curta |
| ONNX/sherpa CPU | 1 chunk / 6min | `43.44s` | `8.29x` | `24` | Mais lento e fragmentou falantes |

## Conclusao

O caminho ONNX/sherpa CPU nao deve virar padrao nem ficar exposto como opcao normal no app. Ele foi mais lento e degradou estabilidade de rotulos no teste curto, alem de nao completar o caso real em 15 minutos.

O melhor caminho medido continua sendo `modern-cpu-chunked`. A proxima investigacao de performance deve focar o backend Python atual: reduzir janelas de embedding com seguranca, persistir/reutilizar estado por worker e melhorar heuristicas de falantes esperados. CUDA/OpenVINO/DirectML so fazem sentido depois de existir uma runtime comprovadamente funcional e benchmarkavel no hardware alvo.
