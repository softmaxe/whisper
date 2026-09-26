# Demo video

A one-minute animated walkthrough of Whisper, written as code with [Remotion](https://www.remotion.dev/). It follows one person through a day of dictation with the Globe/fn key: a Double tap and text cleanup in Mail, a correction that Dictionary learns, a Snippet, Hold mode in a code editor, a batch Upload, History search, Insights, and self-hosted servers in Settings.

The English and Chinese cuts share one timeline and differ only in on-screen text ([`src/copy.ts`](src/copy.ts)). App labels follow [`src/locales`](../src/locales); the Recording pill replays the app's own waveform math from [`waveformMath.ts`](../src/components/dictation/waveformMath.ts).

## Render

This package is separate from the app and has its own dependencies. It needs Node.js 24 and ffmpeg.

```sh
cd demo
npm ci
npm run music
npm run render
```

`npm run music` downloads the instrument samples to `.cache/samples` on first use and writes `public/music.wav`. `npm run render` writes `out/whisper-demo-en.mp4` and `out/whisper-demo-zh-CN.mp4`, each under 10 MB so they can be attached to a README on GitHub. Use `npm run studio` to preview and scrub the timeline in a browser.

Run `npm test` and `npm run typecheck` after changing the timeline, score, or text helpers.

## Timing and music

[`src/timeline.ts`](src/timeline.ts) is the single beat grid: 76 BPM in 4/4, 19 bars, exactly 60 seconds. Scenes and their cues (key presses, pastes, the learned word) are counted in beats, so cuts land on strong beats and [`src/music/score.ts`](src/music/score.ts) can ring a glockenspiel on each paste.

The score is a neo-classical cue in D-flat major: solo piano at dawn, strings entering with the working day, pizzicato through the afternoon, the violins taking the tune at night, and a harp glissando under the logo. [`scripts/render-music.ts`](scripts/render-music.ts) renders it offline with sampled instruments, a hall reverb, and two-pass loudness normalization to -16 LUFS.

To use another track, replace `public/music.wav` and set `BPM` and `MUSIC_OFFSET_SECONDS` in `src/timeline.ts` to the track's tempo and the time of its first downbeat. The picture re-times itself to the new grid.

## Sample credits

- [Salamander Grand Piano](https://archive.org/details/SalamanderGrandPianoV3) by Alexander Holm, CC BY 3.0, via the [Tone.js](https://tonejs.github.io/) sample set.
- [VSCO 2 Community Edition](https://github.com/sgossner/VSCO-2-CE) by Versilian Studios: string sections, harp, and glockenspiel, CC0.
