# Benchmark ASR local gratuito - 2026-05-22

## Escopo

Benchmark isolado, sem integrar backend novo no app.

Head congelado no manifesto: `9d07829462d30ecfccc261dfff1b0d7502649be9`.

Amostras:

- `problematic_2026_05_08`: primeiro chunk da reuniao curta/problemática, 361.41s.
- `fast_2026_05_15`: primeiro chunk da reuniao curta que nao demorava, 357.26s.
- `long_akita_4h46_sample`: primeiro chunk da reuniao longa de 4h46, 358.03s.

Artefatos:

- `benchmarks/runs/asr-candidates-20260522/freeze-manifest.json`
- `benchmarks/runs/asr-candidates-20260522/summary-with-control.csv`

Hardware observado:

- CPU: Intel Core i5-13500T, 14 cores / 20 threads.
- RAM: 16 GB.
- CUDA/NVIDIA: indisponivel (`nvidia-smi` ausente).
- Vulkan: Intel UHD Graphics 770 visivel para `llama.cpp`.

## Resultado de velocidade

| Backend | Modo | problematic | fast | Akita sample | Leitura |
|---|---:|---:|---:|---:|---|
| faster-whisper turbo | CPU int8 | 1.74x | 1.79x | 1.58x | Controle atual local; bom texto, lento. |
| whisper.cpp large-v3-turbo Q5 | BLAS | 2.32x | 2.22x | 2.21x | Mais rapido que faster-whisper, ainda lento para 4h+. |
| whisper.cpp large-v3-turbo Q4 | BLAS | 2.62x | 2.40x | 2.39x | Melhor Whisper local testado; ainda longe do necessario. |
| Qwen3-ASR-0.6B Q8 GGUF | llama.cpp CPU | 2.28x | 2.17x | 1.65x | Nao venceu Whisper Q4; qualidade irregular. |
| Qwen3-ASR-0.6B Q8 GGUF | llama.cpp Vulkan | 2.24x | n/t | n/t | Intel UHD nao ajudou neste teste. |
| Parakeet-TDT-0.6B v3 | direto, chunk inteiro | 5.73x | n/t | n/t | Rapido, mas misturou ingles/portugues no chunk longo. |
| Parakeet-TDT-0.6B v3 | janelas 30s, batch 4 | 13.46x | 13.66x | 12.70x | Melhor candidato. Rapido e texto bem mais estavel. |
| Parakeet-TDT-0.6B v3 | janelas 60s, batch 2 | 10.11x | n/t | n/t | Bom, mas mais lento que 30s e ainda com alguns cortes estranhos. |

`n/t` = nao testado porque a variante ja estava eliminada ou era teste de sanidade.

## Qualidade observada

### faster-whisper turbo

Qualidade conhecida como referencia local. O problema principal e throughput: em CPU ficou entre 1.58x e 1.79x nos chunks medidos.

### whisper.cpp Q4/Q5

Q4 foi mais rapido que Q5 e que faster-whisper. A transcricao manteve o comportamento esperado de Whisper, mas a velocidade ainda projeta uma reuniao de 4h46 para algo em torno de 1h50-2h de transcricao local antes das outras etapas.

### Qwen3-ASR GGUF

Rodou localmente via `llama-mtmd-cli`, mas nao foi competitivo. No caso problematico ficou perto do Whisper Q5 e no trecho Akita caiu para 1.65x. A qualidade tambem ficou menos confiavel em termos de frases tecnicas e texto longo.

### Parakeet direto

O modelo foi rapido no chunk inteiro, mas degradou qualidade em audio de 6 minutos: misturou ingles com portugues. Esse modo nao deve ser usado como backend padrao.

### Parakeet windowed

Dividir em janelas de 30s e rodar em batch 4 mudou o resultado:

- manteve throughput acima de 12x nas tres amostras;
- preservou nomes e contexto no trecho Akita;
- evitou a mistura pesada de ingles/portugues observada no chunk inteiro;
- ainda mostrou pequenas imperfeicoes de fronteira entre janelas.

Para reduzir perda em fronteiras, o proximo teste deve usar janela de 30s com overlap curto, por exemplo 1.0s a 1.5s, e deduplicacao de texto nas bordas.

## OpenVINO/Vulkan

- `llama.cpp` Vulkan encontrou `Intel(R) UHD Graphics 770`, mas Qwen via Vulkan ficou ligeiramente mais lento que CPU.
- `whisper.cpp` BLAS reportou `OPENVINO = 0`; o release Windows baixado nao trouxe OpenVINO ativo.
- Nao havia `cmake`/`cl` no ambiente para compilar uma variante OpenVINO/Vulkan do `whisper.cpp` do zero durante este ciclo.

## Impacto esperado na ata

O impacto final ainda nao foi medido end-to-end dentro do app, porque isso exigiria plugar o transcript novo no pipeline de diarizacao/fatos/ata. Como a regra neste ciclo era nao integrar backend novo, a avaliacao ficou no nivel de ASR.

Mesmo assim, pela qualidade do transcript:

- Qwen GGUF nao deve virar padrao.
- Whisper.cpp Q4 e uma melhoria incremental se quisermos continuar em Whisper local.
- Parakeet windowed e o unico candidato que muda a equacao de tempo sem usar API paga.

## Decisao recomendada

Proximo passo tecnico:

1. Implementar um backend experimental `parakeet-local` isolado, sem substituir o padrao ainda.
2. Usar janelas de 30s, batch 4, modelo carregado uma unica vez.
3. Adicionar overlap curto e deduplicacao na borda.
4. Converter a saida para o formato `TranscriptionSegment` do app.
5. Rodar E2E completo nas duas reunioes reais e comparar ata final contra Groq/faster-whisper.

Se a ata final mantiver qualidade, Parakeet windowed deve substituir `faster-whisper` como backend local gratuito para reunioes longas.
