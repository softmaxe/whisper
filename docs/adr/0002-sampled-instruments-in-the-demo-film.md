# Sampled instruments in the demo Film

The demo Film's score plays its chords and melody on recorded multisample
instruments: an upright piano and a Rhodes. The bass, pad, drums, paper rustle,
and sound effects stay synthesised with numpy. This reverses the earlier rule
that the demo used no samples and downloaded nothing but Remotion's browser.
Synthesised plucked strings and kalimba sounded bright and cold, and a warm,
acoustic score needs recorded instruments.

The specification asks for a felt piano on the chords. No free, downloadable
felt piano SFZ library was found, so an upright piano stands in for it: it is
always struck on its soft velocity layer, scaled back to each note's level,
and its stem is darkened with a low-pass for a felt-like tone.

The samples are not committed and do not use Git LFS. `demo/audio/samples.toml`
pins each library to an immutable archive URL (a commit or dated release) and
its SHA-256. A build step downloads a missing archive into the git-ignored
`demo/.cache/samples/` folder and verifies it. A cached archive is only
verified, so a primed cache builds offline. A failed download or a checksum
mismatch, including a corrupted cached archive, stops the build with the
library's name instead of silently changing the sound. The audio step reads the
SFZ instrument straight from the verified archive and plays it with a minimal
SFZ reader (key, velocity, pitch centre, and release only; release-triggered
regions are skipped and round robins always play their first take).

The music layer receives its melodic instruments through a sample bank. The
build uses the sampled bank by default. The audio tests pass a deterministic
synthesised stand-in, so `pnpm run test:audio` needs neither the cache nor the
network.

GitHub commit archives are generated on demand. If GitHub changes their bytes,
the checksum fails and the manifest must be updated after checking the new
archive. The libraries' licences have not been reviewed; `samples.toml` records
what each source states.

See [Issue #108](https://github.com/softmaxe/whisper/issues/108) and the
[demo redesign specification](https://github.com/softmaxe/whisper/issues/104).
