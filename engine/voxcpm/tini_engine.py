"""Tını voice engine: a persistent sidecar on top of the official VoxCPM (OpenBMB, Apache-2.0).

The Rust core starts this script once; the model is loaded into memory once and request batches from the job queue
are generated in the same process. The protocol is one JSON object per line:

  stdin  <- {"cmd": "load", "model": "<dir>", "precision": "bfloat16|float32", "device": "auto|mps|cpu"}
            {"cmd": "generate", "requests": [...], "out_dir": "<dir>", "cache_dir": "<dir>", "steps": 10, "cfg": 2.0,
             "max_attempts": 3}
            {"cmd": "cancel"}   (while generating; stops before the next request)
            {"cmd": "status"}
            {"cmd": "shutdown"}
  stdout -> {"event": "started" | "ready" | "request_done" | "request_failed" | "batch_done" | "cancelled" | "error"
             | "status", ...}

Request: {"id", "text", "seed", "mode": "design|ultimate|reference", "reference": "<wav>", "reference_text": "..."}
  design    : text in "(description)text" form, no reference (Voice Design)
  ultimate  : the reference is passed as both prompt and reference, with its exact transcript (Ultimate / Hi-Fi cloning)
  reference : reference only (controllable cloning; the text may carry a "(style)" prefix)

stdout is reserved for the protocol; everything libraries print goes to stderr.
"""

import hashlib
import json
import os
import queue
import sys
import threading
import time
import traceback
import warnings

warnings.filterwarnings("ignore", category=FutureWarning)
warnings.filterwarnings("ignore", category=UserWarning)

# Protocol channel: keep the real stdout for ourselves and point the process stdout at stderr, so library output
# cannot corrupt the protocol.
_proto = os.fdopen(os.dup(1), "w", encoding="utf-8", buffering=1)
os.dup2(2, 1)
sys.stdout = sys.stderr
os.environ.setdefault("TOKENIZERS_PARALLELISM", "false")
os.environ.setdefault("PYTORCH_ENABLE_MPS_FALLBACK", "1")
os.environ.setdefault("HF_HUB_OFFLINE", "1")
os.environ.setdefault("TQDM_DISABLE", "1")

VERSION = 1
_emit_lock = threading.Lock()


def emit(**event):
    with _emit_lock:
        _proto.write(json.dumps(event, ensure_ascii=False) + "\n")
        _proto.flush()


def log(*a):
    print("[tini_engine]", *a, file=sys.stderr, flush=True)


# --- command reader: a separate thread so "cancel" is seen while a batch is generating -----------------------------
_commands: "queue.Queue[dict | None]" = queue.Queue()
_cancel = threading.Event()


def _reader():
    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        try:
            c = json.loads(line)
        except json.JSONDecodeError as e:
            emit(event="error", message=f"geçersiz JSON: {e}")
            continue
        if c.get("cmd") == "cancel":
            _cancel.set()
            continue
        _commands.put(c)
    _commands.put(None)  # stdin closed: the parent process is gone


threading.Thread(target=_reader, daemon=True).start()


# --- model --------------------------------------------------------------------------------------------------------
class Engine:
    def __init__(self):
        self.m = None
        self.tm = None
        self.model_path = None
        self.precision = None
        self.prompt_caches: dict[str, dict] = {}

    def load(self, model: str, precision: str, device: str):
        if self.m is not None and self.model_path == model and self.precision == precision:
            emit(event="ready", device=str(self.tm.device), precision=self.precision, seconds=0.0, version=VERSION,
                 reloaded=False)
            return
        # VoxCPM defaults to float32 on MPS; bfloat16 is chosen explicitly for memory and speed.
        os.environ["VOXCPM_MPS_DTYPE"] = precision
        t = time.time()
        import torch  # noqa: F401  (counted in load time)
        from voxcpm import VoxCPM

        self.m = None
        self.tm = None
        self.prompt_caches.clear()
        torch_device = None if device in ("", "auto", None) else device
        self.m = VoxCPM.from_pretrained(
            model, load_denoiser=False, optimize=False, local_files_only=True, device=torch_device
        )
        self.tm = self.m.tts_model
        self.model_path = model
        self.precision = precision
        emit(
            event="ready",
            device=str(self.tm.device),
            precision=str(next(self.tm.base_lm.parameters()).dtype).replace("torch.", ""),
            seconds=round(time.time() - t, 1),
            version=VERSION,
            sample_rate=self.tm.sample_rate,
            reloaded=True,
        )

    # Prompt cache: the reference encoding (AudioVAE) runs once per voice and is kept in memory and on disk.
    def prompt_cache(self, mode: str, reference: str, reference_text: str, cache_dir: str | None):
        import torch

        with open(reference, "rb") as f:
            digest = hashlib.sha1(f.read()).hexdigest()
        key = hashlib.sha1(f"{mode}|{digest}|{reference_text}|v{VERSION}".encode()).hexdigest()[:24]
        if key in self.prompt_caches:
            return self.prompt_caches[key]
        path = os.path.join(cache_dir, f"{key}.pt") if cache_dir else None
        cache = None
        if path and os.path.isfile(path):
            try:
                cache = torch.load(path, map_location="cpu", weights_only=True)
            except Exception as e:  # corrupt cache: encode again
                log("prompt cache unreadable", path, e)
        if cache is None:
            if mode == "ultimate":
                cache = self.tm.build_prompt_cache(
                    prompt_text=reference_text, prompt_wav_path=reference, reference_wav_path=reference
                )
            else:
                cache = self.tm.build_prompt_cache(reference_wav_path=reference)
            if path:
                os.makedirs(cache_dir, exist_ok=True)
                torch.save(cache, path + ".tmp")
                os.replace(path + ".tmp", path)
        if len(self.prompt_caches) >= 8:
            self.prompt_caches.pop(next(iter(self.prompt_caches)))
        self.prompt_caches[key] = cache
        return cache

    def generate(self, c: dict):
        import numpy as np
        import soundfile as sf
        import torch

        requests = c.get("requests") or []
        out_dir = c["out_dir"]
        os.makedirs(out_dir, exist_ok=True)
        steps = int(c.get("steps", 10))
        cfg = float(c.get("cfg", 2.0))
        max_attempts = int(c.get("max_attempts", 3))
        start = time.time()
        _cancel.clear()
        for r in requests:
            if _cancel.is_set():
                emit(event="cancelled")
                return
            t = time.time()
            try:
                text = " ".join(str(r["text"]).split())
                if not text:
                    raise ValueError("boş metin")
                mode = r.get("mode", "design")
                if mode == "design":
                    cache = None
                else:
                    cache = self.prompt_cache(mode, r["reference"], r.get("reference_text", ""), c.get("cache_dir"))
                torch.manual_seed(int(r.get("seed", 42)) & 0xFFFFFFFF)
                # Retries are done here (not by VoxCPM) so the attempt count can be reported.
                target_tokens = len(self.tm.text_tokenizer(text))
                wav = None
                attempt = 0
                for attempt in range(1, max_attempts + 1):
                    w, _, feat = self.tm.generate_with_prompt_cache(
                        target_text=text,
                        prompt_cache=cache,
                        inference_timesteps=steps,
                        cfg_value=cfg,
                        retry_badcase=False,
                    )
                    wav = w
                    if feat.shape[0] < target_tokens * 6.0:
                        break
                    log(f"{r['id']}: bad case (audio/text ratio {feat.shape[0] / max(target_tokens, 1):.1f}), "
                        f"attempt {attempt}")
                    if _cancel.is_set():
                        break
                audio = wav.squeeze(0).float().cpu().numpy().astype(np.float32)
                path = os.path.join(out_dir, f"{r['id']}.wav")
                sf.write(path, audio, self.tm.sample_rate, subtype="FLOAT")
                gpu_mb = None
                if torch.backends.mps.is_available():
                    gpu_mb = round(torch.mps.driver_allocated_memory() / 2**20)
                emit(
                    event="request_done",
                    gpu_mb=gpu_mb,
                    id=r["id"],
                    wav=path,
                    audio_s=round(len(audio) / self.tm.sample_rate, 3),
                    gen_s=round(time.time() - t, 2),
                    attempts=attempt,
                    complete=bool(feat.shape[0] < target_tokens * 6.0),
                )
            except Exception as e:
                log(traceback.format_exc())
                emit(event="request_failed", id=r.get("id"), message=f"{type(e).__name__}: {e}")
        emit(event="batch_done", seconds=round(time.time() - start, 2))


def main():
    engine = Engine()
    emit(event="started", version=VERSION, python=sys.version.split()[0], pid=os.getpid())
    while True:
        c = _commands.get()
        if c is None or c.get("cmd") == "shutdown":
            break
        try:
            cmd = c.get("cmd")
            if cmd == "load":
                engine.load(c["model"], c.get("precision", "bfloat16"), c.get("device", "auto"))
            elif cmd == "generate":
                if engine.tm is None:
                    emit(event="error", message="model yüklenmedi")
                    continue
                engine.generate(c)
            elif cmd == "status":
                import torch

                info = {"loaded": engine.tm is not None}
                if torch.backends.mps.is_available():
                    info["mps_mb"] = round(torch.mps.current_allocated_memory() / 2**20)
                emit(event="status", **info)
            else:
                emit(event="error", message=f"bilinmeyen komut: {cmd}")
        except Exception as e:
            log(traceback.format_exc())
            emit(event="error", message=f"{type(e).__name__}: {e}")


if __name__ == "__main__":
    main()
