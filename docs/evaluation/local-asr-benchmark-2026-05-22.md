# Local ASR Benchmark - 2026-05-22

Audio: `C:\Users\User\Downloads\FABIO AKITA - Flow #588 - Flow Podcast (youtube).mp3`

Duration: `04:44:12.43`.

Sample: first 3 app chunks from run `694e8725-5c83-477f-846b-bd683c3349b9`, `1059.02s` of audio.

Machine result: CTranslate2 reported `cuda_devices=0`, so all local runs used CPU.

| Backend | Model | Device / compute | Audio | Wall clock | Speed | Quality note |
| --- | --- | --- | ---: | ---: | ---: | --- |
| Groq saved run | `whisper-large-v3-turbo` | remote | `7126.72s` before quota | UI showed `215.5x` | `~215x` | Good enough for current pipeline, but hit free ASH limit at 20/48 chunks. |
| faster-whisper | `turbo` | CPU `int8` | `1059.02s` | `876.65s` | `1.21x` | Best local quality tested; too slow on this CPU for 4h+ meetings. |
| faster-whisper | `turbo` | CPU `int8`, warmed first chunk, batch 8 | `358.03s` | `245.05s` | `1.46x` | Cache warm does not solve the CPU bottleneck. |
| faster-whisper | `turbo` | CPU `int8`, warmed first chunk, batch 1 | `358.03s` | `230.17s` | `1.56x` | Slightly faster, but creates many more segments. |
| faster-whisper | `distil-large-v3` | CPU `int8` | `1059.02s` | `831.67s` | `1.27x` | Not materially faster than turbo here. |
| faster-whisper | `small` | CPU `int8` | `1059.02s` | `246.40s` | `4.30x` | Too many domain errors: IA, Claude, benchmark, model names, and technical terms. |

## Conclusion

`faster-whisper turbo` is the correct free/local fallback for quality, but this CPU-only machine cannot match Groq speed. A 4h44 meeting would project to roughly 3h55 with `turbo` on CPU, or roughly 66 minutes with `small` while losing important technical terms.

The practical free path is:

1. Use `faster-whisper turbo` local when Groq quota is unavailable or when strict no-cloud mode is required.
2. Prefer CUDA/GPU for local ASR if available.
3. Do not replace the default short-meeting Groq path solely with CPU local ASR on this machine.
4. Do not use `small` as the default for meeting minutes unless the user explicitly chooses speed over transcript quality.
