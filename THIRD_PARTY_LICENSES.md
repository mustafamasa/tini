# Third-party components and attributions

Tını's own code is licensed under Apache-2.0 (see `LICENSE`).

| Component | How it is used | License | Source |
|---|---|---|---|
| VoxCPM2 (OpenBMB) | Speech model, downloaded on first run (`openbmb/VoxCPM2`, pinned commit) | Apache-2.0 | https://huggingface.co/openbmb/VoxCPM2 |
| VoxCPM 2.0.3 (OpenBMB) | Inference library, installed into the app's private Python environment | Apache-2.0 | https://github.com/OpenBMB/VoxCPM |
| PyTorch, torchaudio | Runtime of the engine (MPS) | BSD-3-Clause | https://pytorch.org |
| transformers, tokenizers, safetensors, huggingface-hub, einops, librosa, soundfile, numpy, pydantic, regex, tqdm | Engine dependencies (PyPI) | Apache-2.0 / MIT / ISC / BSD | https://pypi.org |
| Python 3.12 (python-build-standalone) | Engine interpreter, downloaded during setup | PSF-2.0 | https://github.com/astral-sh/python-build-standalone |
| uv (Astral) | Installs the Python environment; bundled binary | MIT / Apache-2.0 | https://github.com/astral-sh/uv |
| whisper.cpp v1.9.5 (ggml-org) | Transcription and word alignment; bundled binary | MIT | https://github.com/ggml-org/whisper.cpp |
| OpenAI Whisper large-v3-turbo (ggml q5_0) | Transcription model, downloaded on demand | MIT | https://huggingface.co/ggerganov/whisper.cpp |
| voxcpm2-turkce-ses-tasarimi (Ali Haydar Öztürk) | 144 sample voice prompts, Turkish pronunciation dictionary; app behavior was modeled on this project | Apache-2.0 | https://github.com/alexturkey/voxcpm2-turkce-ses-tasarimi |
| flag-icons 7.2.3 (Panayiotis Lipiridis) | 30 flag SVGs | MIT | https://github.com/lipis/flag-icons |
| LAME | MP3 export (via mp3lame-sys) | LGPL-2.0 | https://lame.sourceforge.io |
| ffmpeg | Optional, used for muxing into video if installed on the system; not bundled | LGPL/GPL | https://ffmpeg.org |
| Tauri, React, CodeMirror, wavesurfer.js, lucide, zustand | App shell and UI | MIT / Apache-2.0 / ISC / BSD-3-Clause | npm, crates.io |
| Rust crates (rusqlite/SQLite, symphonia, hound, realfft, serde, ureq, …) | Core library | MIT / Apache-2.0 / MPL-2.0 (symphonia) / public domain (SQLite) | crates.io |

## Built-in voices

`resources/voices/cansu` and `resources/voices/gozde` are synthetic voices created with Tını's voice design (VoxCPM2). Their reference recordings are model output, not recordings of real people, and are distributed under the project's license (Apache-2.0). See `resources/voices/README.md`.

## Responsible use

- Generated speech is synthetic. Label it as AI-generated when you publish it; Tını writes a "synthetic" tag into every export.
- Clone a voice only with your own voice or the speaker's explicit permission. The app requires a consent confirmation; do not remove that gate.
- Impersonation, fraud and deception are prohibited.

Tını is not affiliated with OpenBMB or ModelBest.
