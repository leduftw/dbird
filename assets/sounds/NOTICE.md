The five sound-effect files in this directory are original works created
specifically for dbird:

- `sfx_die.ogg`
- `sfx_hit.ogg`
- `sfx_point.ogg`
- `sfx_swooshing.ogg`
- `sfx_wing.ogg`

They are synthesized entirely from scratch by [`generate.py`](generate.py) in
this directory — plain oscillators, filtered noise, and envelopes — and contain
no third-party recordings or samples. Running `python3 generate.py` (with
`oggenc` from vorbis-tools, or `ffmpeg`, on PATH) regenerates them
deterministically.

These files are covered by the same license as the rest of the repository.
