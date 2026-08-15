# Bundled castle environment

These files are the reproducible, repository-hosted castle-room fallback used
by a clean installation. They do not contain HeroQuest scans or third-party 3D
models.

- `castle-great-hall-reference.png` was generated with OpenAI ImageGen for this
  project as the art-direction reference.
- `castle-great-hall-matte-v1.png` was an ImageGen edit that removed the table
  from the reference.
- `castle-great-hall-panorama-v1.png` was generated from the same reference as
  a 2:1 spherical room panorama.
- `castle-great-hall-panorama-v1-4x.png` is that panorama enlarged locally with
  Real-ESRGAN `realesrgan-x4plus`.
- `castle-great-hall.blend`, `castle-great-hall.glb`, the preview, and the four
  texture PNGs were built locally by `tools/build-castle-room.py`.

The matching working copies under `assets/local/` remain optional overrides.
The checked-in files ensure that the room never depends on an unavailable AI
session, API key, or generated-image cache at first run.
