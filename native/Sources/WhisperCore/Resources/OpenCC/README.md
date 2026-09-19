# OpenCC conversion data

These unmodified dictionary sources are from opencc-data 1.4.1 at
`c7b739cec68223304b49ac89f181e58802767d0b`, generated upstream from OpenCC
`81223ed87ae53283ef518e2deac34b7971f8a39e`. `provenance.json` records exact URLs
and SHA-256 hashes. Data is licensed under Apache-2.0; preserve the included
license and each source header.

Whisper's Swift conversion engine adapts the MIT-licensed opencc-js 1.4.1 engine
at `c410ad49ce6565c007a08faa20454863d2aaff26`. Its MIT notice is included here.
At load time the Swift adapter chooses each entry's first target and removes
only single-UTF16-unit identity entries, matching that package's build rules.
The `s2twp` and `tw2sp` pipelines include compatibility normalization and fixed
phrase segmentation. `TSCharactersExt` is intentionally excluded, matching the
JavaScript package's `may_output_tofu` filter. Dictionary bytes are unchanged.

No JavaScript engine or runtime is included in the native app.
