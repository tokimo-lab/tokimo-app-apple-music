---
name: download-music
description: "Download an Apple Music song/track to local cache. Use to find a track by name (or via an album's track list), confirm it, and download the decrypted audio file at a chosen quality."
when-to-use: "When the user wants to download/save an Apple Music song or track to local files."
argument-hint: "<song name or track id>"
version: "0.1.0"
context: inline
---

# Download Apple Music Song

Find a track's catalog ID, then download the decrypted audio to disk.

## Prerequisites

- The user must be **logged into Apple Music** in the app (storefront set) for catalog access.
- Downloading requires an **active Apple Music subscription**.
- The `<track_id>` passed to `download` must be a **SONG id** (not an album or artist id).

## Quick Reference

| Step | Command |
|------|---------|
| Confirm login (optional) | `tokimo-app-apple-music status` |
| Find a song's ID | `tokimo-app-apple-music search "<song name>" --types songs` |
| Get IDs from an album's tracks | `tokimo-app-apple-music album <album_id>` |
| Confirm the track (optional) | `tokimo-app-apple-music song <track_id>` |
| Download | `tokimo-app-apple-music download <track_id> --output <path> --quality <lossless\|high\|standard>` |

## Workflow

1. **(Optional) Confirm login.**

   ```bash
   tokimo-app-apple-music status
   ```

2. **Find the track ID.** Search for songs and read the `ID` column.

   ```bash
   tokimo-app-apple-music search "<song or artist name>" --types songs
   ```

   The songs table columns are: `ID, Name, Artist, Album, Duration, Composer, Genre`.

   Alternatively, take a track ID from an album's track list (each track has its own ID):

   ```bash
   tokimo-app-apple-music album <album_id>
   ```

3. **(Optional) Confirm the right track.**

   ```bash
   tokimo-app-apple-music song <track_id>
   ```

4. **Download the track.**

   ```bash
   tokimo-app-apple-music download <track_id> --output <path> --quality high
   ```

   - `--output` — destination path. Default `.` (current directory).
   - `--quality` — `lossless`, `high`, or `standard`. Default `high`.

   The track is downloaded and decrypted to the output path (e.g. an `.m4a` file).

## Worked Example

Download "Blank Space":

```bash
# 1. Find the song ID
tokimo-app-apple-music search "Blank Space" --types songs
#   -> ID 1440935808  Name "Blank Space"  Artist "Taylor Swift" ...

# 2. (optional) Confirm
tokimo-app-apple-music song 1440935808

# 3. Download in lossless quality
tokimo-app-apple-music download 1440935808 --output ./blank-space.m4a --quality lossless
```

## Notes

- `--types` for `search` defaults to `songs,albums`; pass `--types songs` to get only songs (which carry downloadable track IDs).
- Album and artist IDs cannot be passed to `download` — resolve them to a song ID first via `album <album_id>` or `search --types songs`.
