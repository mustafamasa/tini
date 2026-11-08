# Built-in voices

These voices ship with Tını and appear in every user's library as read-only `builtin/<name>` voices.

| Voice | Language | Kind | Reference |
|---|---|---|---|
| Cansu (`cansu/`) | Turkish | Synthetic voice made with Tını's voice design (VoxCPM2) | 25 s, `reference.wav` + transcript in `voice.json` |
| Gözde (`gozde/`) | Turkish | Synthetic voice made with Tını's voice design (VoxCPM2) | 27 s, `reference.wav` + transcript in `voice.json` |

**Origin.** Both voices were created from a text description with Tını's voice design, which runs VoxCPM2. They are
not recordings or clones of real people; any resemblance to a real person's voice is coincidental. The reference
recordings are the model's own output and are distributed under the project's license.

**Labeling.** Speech generated with these voices is synthetic. Label it as AI-generated when you publish it.

## Adding a built-in voice

1. Add a folder with `reference.wav` (5–30 s, clean, no background noise) and a `voice.json` card. The card needs at
   least `name`, `display_name`, `language`, `design_text` (the exact transcript of the recording), `read_seed`,
   `reference`, and a `distribution` note that says where the voice comes from.
2. Prefer voices made with voice design. A real person's voice may only be added with the speaker's written permission
   for public distribution.
3. Measure the reference before adding it. Its noise floor should be well below the speech: about 50 dB or more, as
   in the existing voices.
