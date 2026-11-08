# Tını brand

The primary logo is the **t icon** next to the lowercase **tını** wordmark.

| File | Use |
|---|---|
| `tini-icon.svg` | macOS app icon (1024 canvas, 824 squircle with system padding). The source of `app/design/icon.png` and `app/src-tauri/icons/*`. |
| `tini-mark-on-dark.svg`, `tini-mark-on-light.svg` | The mark without a container, for dark and light backgrounds. |
| `tini-wordmark-dark.svg`, `tini-wordmark-light.svg` | The lowercase wordmark alone. Use the dark file on light backgrounds and the light file on dark backgrounds. |
| `tini-lockup-dark.svg`, `tini-lockup-light.svg` | The primary horizontal lockup: icon and wordmark. |
| `tini-icon-sampling.svg` | Secondary element: the wave dissolving into three sampled dots ("natural voice becomes digital"). Use it for animation and supporting graphics, never as the app icon. |

Regenerate every file after a change with:

```bash
python3 docs/brand/generate.py
```

The script uses only the Python standard library.

## Construction

Everything is drawn on a 160 × 160 grid.

- **Stem.** The t stem is 18 units wide with round terminals. It runs from y = 24 down to a hook that ends with a slight upturn, echoing the wave.
- **Wave.** The wave is a filled shape, not a stroke. It follows 1.25 periods of a slightly chirped sine with an asymmetric envelope. It is 2.6 units wide at the ends and about 7.6 units wide in the middle, and both ends close with half-circle arcs.
- **Weave.** The wave crosses the stem exactly at a zero crossing. A 3-unit gap in the background color separates the two, so the wave reads as woven through the letter.
- **Wordmark.** The wordmark reuses the icon's t exactly: same stem, hook and stroke weight. Its x-height sits on the icon's wave line (y = 66).
  - The arch of the n is a true half-circle.
  - Inter-letter gaps are narrower than the n's counter, so the two dotless ı read as separate letters.

## Colors

| Name | Hex | Role |
|---|---|---|
| Ink | `#1F1D2B` | Icon background, text on light |
| Cream | `#F5F1EA` | Letterform on dark, light background |
| Coral | `#F0997B` | Wave on dark |
| Deep coral | `#D85A30` | Wave on light, accent background |

Alternative applications:
- **Light:** cream background, ink letter, deep-coral wave.
- **Accent:** deep-coral background, cream letter and wave.

## Usage

- **Clear space:** keep at least the stem width × 2 free on every side of the icon and the lockup.
- **Minimum sizes:** 16 px for the icon. For the wordmark, a 10 px x-height.
- **Don't** rotate, outline or recolor parts of the logo outside the palette above. Don't add the three dots to the app icon, and don't set the wordmark in a typeface.

## Animation (sampling dots)

Use the animation for "generating" states and the splash screen.

1. The mark appears.
2. The three dots fade in left to right, 120 ms apart (opacity 0 → 1 and scale 0.6 → 1, `ease-out`, 180 ms each).
3. The dots hold for 600 ms.
4. They fade out together.

Respect `prefers-reduced-motion`: when it is set, show the static mark.
