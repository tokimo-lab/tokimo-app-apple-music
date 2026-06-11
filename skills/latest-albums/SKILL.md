---
name: latest-albums
description: "Find an artist's latest/newest albums on Apple Music. Resolve the artist ID, then list their albums (sorted newest-first) to read the most recent releases off the top rows."
when-to-use: "When the user asks for an artist's latest/newest albums or recent releases on Apple Music."
argument-hint: "<artist name>"
version: "0.1.0"
context: inline
---

# Latest Albums for an Artist

There is **no** "new releases" / "charts" / "latest" subcommand. To get an
artist's newest albums, resolve the artist, then list their albums — that list
is sorted by release date **descending**, so the top rows are the latest.

## Prerequisites

- The user must be **logged into Apple Music** in the app (storefront set) for catalog access. Verify with `status`.

## `--region` Option

All subcommands (`search`, `album`, `song`, `artist`, `download`) accept an optional `--region <code>` (or `-r`) flag to browse a different region's catalog (e.g. `--region jp`, `--region cn`).

- If omitted, the user's account storefront is used (default behavior).
- If the specified region differs from the account region, a warning is printed: music in the browsing region is **view-only and cannot be downloaded**.

## Quick Reference

| Step | Command |
|------|---------|
| Confirm login (optional) | `tokimo-app-apple-music status` |
| Find the artist ID | `tokimo-app-apple-music search "<artist name>" --types artists` |
| List albums (newest-first) | `tokimo-app-apple-music artist <artist_id>` |
| Album details (optional) | `tokimo-app-apple-music album <album_id>` |
| Keyword album search (alt) | `tokimo-app-apple-music search "<keyword>" --types albums` |

> **Tip:** Add `--region <code>` (e.g. `--region jp`) to any command to browse a different region's catalog. See [Region Option](#--region-option) for details.

## Workflow

1. **Find the artist ID.** Search with `--types artists` and read the `ID` column.

   ```bash
   tokimo-app-apple-music search "<artist name>" --types artists
   ```

   The artists table columns are: `ID, Name, Genre`.

2. **List the artist's albums (newest-first).**

   ```bash
   tokimo-app-apple-music artist <artist_id>
   ```

   The album list is sorted by release date **descending** — the **top rows are
   the latest releases**. Columns: `ID, Tracks, Released, Name`.

3. **(Optional) Inspect a specific album.**

   ```bash
   tokimo-app-apple-music album <album_id>
   ```

## Worked Example

Latest albums by Taylor Swift:

```bash
# 1. Resolve the artist ID
tokimo-app-apple-music search "Taylor Swift" --types artists
#   -> ID 159260351  Name "Taylor Swift" ...

# 2. List albums; newest releases are at the top
tokimo-app-apple-music artist 159260351

# 3. (optional) Look at the newest album's tracks
tokimo-app-apple-music album <top_album_id>
```

## Alternative: Keyword Album Search

To search albums by keyword instead of by artist:

```bash
tokimo-app-apple-music search "<keyword>" --types albums
```

Albums table columns: `ID, Name, Artist, Tracks, Released, Genre`.

> Note: keyword album search results are **not strictly sorted by date**. For a
> reliable newest-first ordering, use `artist <artist_id>`.
