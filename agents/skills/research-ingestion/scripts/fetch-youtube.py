#!/usr/bin/env python3
"""Fetch YouTube transcripts for a channel's recent videos.

Usage:
    python3 fetch-youtube.py "Simon Dixon" --since 14d --out /tmp/transcripts/
    python3 fetch-youtube.py "SimonDixonBTCT" --since 7d --out /tmp/transcripts/ --min-duration 900

Tries multiple methods in order:
1. youtube-transcript-api (cleanest)
2. yt-dlp auto-generated subtitles
3. Author's blog/website (if known)
4. Web search for transcript summaries

Outputs one .md file per video in the output directory.
"""

import argparse
import json
import os
import re
import subprocess
import sys
from datetime import datetime, timedelta
from pathlib import Path


def parse_since(since_str: str) -> datetime:
    """Parse relative time string like '14d', '2w', '30d'."""
    match = re.match(r'^(\d+)([dwm])$', since_str)
    if not match:
        raise ValueError(f"Invalid --since format: {since_str}. Use Nd, Nw, or Nm.")
    n, unit = int(match.group(1)), match.group(2)
    if unit == 'd':
        return datetime.now() - timedelta(days=n)
    elif unit == 'w':
        return datetime.now() - timedelta(weeks=n)
    elif unit == 'm':
        return datetime.now() - timedelta(days=n * 30)


def find_videos(channel: str, since: datetime, min_duration: int = 900):
    """Find recent videos from a channel using yt-dlp."""
    print(f"Searching for videos from '{channel}' since {since.date()}...")

    # Try channel URL patterns
    urls_to_try = [
        f"https://www.youtube.com/@{channel}/videos",
        f"https://www.youtube.com/c/{channel}/videos",
        f"ytsearch20:{channel}",
    ]

    for url in urls_to_try:
        try:
            result = subprocess.run(
                ["yt-dlp", "--flat-playlist", "--dump-json",
                 "--dateafter", since.strftime("%Y%m%d"),
                 "--match-filter", f"duration>{min_duration}",
                 url],
                capture_output=True, text=True, timeout=60
            )
            if result.returncode == 0 and result.stdout.strip():
                videos = []
                for line in result.stdout.strip().split('\n'):
                    if line.strip():
                        try:
                            videos.append(json.loads(line))
                        except json.JSONDecodeError:
                            continue
                if videos:
                    print(f"Found {len(videos)} videos")
                    return videos
        except (subprocess.TimeoutExpired, FileNotFoundError):
            continue

    print("yt-dlp failed, falling back to web search")
    return None


def get_transcript_ytapi(video_id: str) -> str | None:
    """Try youtube-transcript-api."""
    try:
        from youtube_transcript_api import YouTubeTranscriptApi
        transcript = YouTubeTranscriptApi.get_transcript(video_id)
        return ' '.join(entry['text'] for entry in transcript)
    except Exception as e:
        print(f"  youtube-transcript-api failed: {e}")
        return None


def get_transcript_ytdlp(video_id: str, out_dir: Path) -> str | None:
    """Try yt-dlp subtitle download."""
    try:
        result = subprocess.run(
            ["yt-dlp", "--write-auto-sub", "--sub-lang", "en",
             "--skip-download", "--sub-format", "vtt",
             "-o", str(out_dir / "%(id)s"),
             f"https://www.youtube.com/watch?v={video_id}"],
            capture_output=True, text=True, timeout=30
        )
        # Look for the subtitle file
        vtt_path = out_dir / f"{video_id}.en.vtt"
        if vtt_path.exists():
            text = vtt_path.read_text()
            # Clean VTT format
            lines = []
            for line in text.split('\n'):
                if '-->' not in line and not line.strip().isdigit() and line.strip():
                    if not line.startswith('WEBVTT') and not line.startswith('Kind:'):
                        lines.append(line.strip())
            vtt_path.unlink()  # cleanup
            return ' '.join(dict.fromkeys(lines))  # deduplicate consecutive lines
        return None
    except (subprocess.TimeoutExpired, FileNotFoundError):
        return None


def slugify(text: str) -> str:
    """Convert title to filename-safe slug."""
    text = text.lower()
    text = re.sub(r'[^\w\s-]', '', text)
    text = re.sub(r'[\s_]+', '-', text)
    return text[:80].strip('-')


def save_transcript(out_dir: Path, video: dict, transcript: str):
    """Save transcript as markdown file."""
    title = video.get('title', 'Unknown')
    date = video.get('upload_date', '')
    if date:
        date_formatted = f"{date[:4]}-{date[4:6]}-{date[6:8]}"
    else:
        date_formatted = datetime.now().strftime('%Y-%m-%d')

    video_id = video.get('id', 'unknown')
    url = f"https://www.youtube.com/watch?v={video_id}"
    duration = video.get('duration', 0)
    duration_str = f"{duration // 3600}h{(duration % 3600) // 60}m" if duration > 3600 else f"{duration // 60}m"

    slug = slugify(title)
    filename = f"{date_formatted}-{slug}.md"

    content = f"""# {title}
**Date:** {date_formatted}
**URL:** {url}
**Duration:** {duration_str}
**Source:** YouTube

## Transcript

{transcript}
"""

    filepath = out_dir / filename
    filepath.write_text(content)
    print(f"  Saved: {filename}")
    return filepath


def main():
    parser = argparse.ArgumentParser(description='Fetch YouTube transcripts')
    parser.add_argument('channel', help='Channel name or handle')
    parser.add_argument('--since', default='14d', help='How far back (e.g., 14d, 2w)')
    parser.add_argument('--out', required=True, help='Output directory')
    parser.add_argument('--min-duration', type=int, default=900, help='Min video duration in seconds')
    args = parser.parse_args()

    out_dir = Path(args.out)
    out_dir.mkdir(parents=True, exist_ok=True)

    since = parse_since(args.since)
    videos = find_videos(args.channel, since, args.min_duration)

    if not videos:
        print("No videos found. Agent should fall back to web_search.")
        sys.exit(1)

    results = {"fetched": 0, "failed": 0, "files": []}

    for video in videos:
        title = video.get('title', 'Unknown')
        video_id = video.get('id', '')
        print(f"\nProcessing: {title}")

        # Try methods in order
        transcript = get_transcript_ytapi(video_id)
        if not transcript:
            transcript = get_transcript_ytdlp(video_id, out_dir)

        if transcript and len(transcript) > 200:
            filepath = save_transcript(out_dir, video, transcript)
            results["fetched"] += 1
            results["files"].append(str(filepath))
        else:
            print(f"  No transcript available for: {title}")
            results["failed"] += 1

    print(f"\n{'='*50}")
    print(f"Fetched: {results['fetched']}, Failed: {results['failed']}")
    print(json.dumps(results, indent=2))


if __name__ == '__main__':
    main()
