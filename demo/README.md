# Demo video

A 75-second hand-drawn explainer of Whisper, written as code with [Remotion](https://www.remotion.dev/). Whis, Whisper's mascot, takes the viewer through five Beats on textured paper with coloured-pencil strokes, red-pen annotations, and handwritten Captions:

1. **Opening**: typing is slow; the Globe/fn key is circled and a Double tap brings up the Recording pill.
2. **Speak and it's written**: fillers are crossed out and punctuation added before the text lands in Mail by Automatic paste; a misheard product name is corrected in team chat and written into Dictionary.
3. **More ways to use it**: a Snippet, Hold mode in a code editor, and a batch Upload.
4. **The day in review**: the paper washes to night; History search highlights matches and Insights are drawn by hand.
5. **Your servers**: the Mac and the speech and cleanup servers share one house, and data never leaves it; then the logo, the install command, and the repository link.

The English and Chinese cuts share one timeline and differ only in on-screen text. App labels in [`src/copy.ts`](src/copy.ts) follow [`src/locales`](../src/locales); the Recording pill replays the app's own waveform math from [`waveformMath.ts`](../src/components/dictation/waveformMath.ts).

## Build

This package is separate from the app and has its own dependencies. It needs Node.js 24, [uv](https://docs.astral.sh/uv/), and ffmpeg.

```sh
cd demo
npm ci
npm run build
```

`npm run build` exports the timeline in [`timeline/`](timeline) to `out/timeline.json`, synthesises `out/audio.wav` with the Python package in [`audio/`](audio), renders the `Film-en` and `Film-zh` compositions, muxes them into `out/whisper-film-en.mp4` and `out/whisper-film-zh-CN.mp4`, and extracts PNG review frames at Beat midpoints, Caption ends, and key moments to `out/frames/<lang>/`. It also renders the `WhisSheet` model sheet of Whis to `out/frames/whis-sheet.png`. Pass `-- --concurrency=N` to limit render threads. The first render downloads Chrome Headless Shell for Remotion; nothing else is downloaded. Use `npm run studio` to preview and scrub the Film in a browser.

## Timeline and sound

Each Beat has one timeline module in [`timeline/beats/`](timeline/beats) that defines its window, named moments, Captions (text per language, time, and position), and sound cues. The picture imports these modules; the audio step reads the exported JSON. Change a time in one place and picture and sound stay in sync.

The score and sound effects are synthesised with numpy, with no samples: plucked strings (Karplus-Strong) and kalimba at about 84 BPM, with brushed drums, paper rustle, and a soft pad at night, arranged by Beat and resolving on the home chord. Each sound cue type (key click, recording start and stop chimes, red-pen scratch, tick, paste, file drop) has its own synthesiser module in [`audio/whisper_audio/sfx/`](audio/whisper_audio/sfx).

## Tests

- `npm test`: timeline rules (Caption length and overlap, Beat coverage, moment and cue order), Caption layout in both languages, and font coverage of every handwritten character.
- `npm run test:audio`: the synthesiser writes a WAV of the right length with sound at every cue.
- `npm run test:film`: both rendered cuts' duration, resolution, frame rate, codecs, streams, music level, final fade, and blank last second. It rebuilds the cuts first when they are missing or stale.
- `npm run typecheck`.

## Credits

- Captions and annotations use [LXGW WenKai](https://github.com/lxgw/LxgwWenKai) under the SIL Open Font License ([`public/fonts/OFL.txt`](public/fonts/OFL.txt)), bundled as a subset of ASCII, common CJK punctuation, and GB2312 made by [`scripts/subset_font.py`](scripts/subset_font.py).
- Strokes are drawn with [Rough.js](https://roughjs.com/) (MIT).
