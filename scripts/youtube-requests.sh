#!/usr/bin/env bash
# Take YouTube URLs to HLS unattended: docs/operations.md, "From a YouTube URL to HLS, unattended".
# Usage: scripts/youtube-requests.sh [--accept-provisional] URL [URL ...]
#        scripts/youtube-requests.sh status   where each recording is, with step percentages
#        scripts/youtube-requests.sh watch    wait for the next new events, print them and exit
set -euo pipefail
repo="$(cd "$(dirname "$0")/.." && pwd -P)"
cd "$repo"
jobs=edits/work/youtube-intake
queue=edits/work/recording-pipeline/requested-youtube-queue.json
defaults=edits/work/recording-pipeline/DEFAULTS-v1.json
export PATH="$repo/.venv/bin:$PATH" PYTHONPATH=src
mkdir -p "$jobs"
watching=(.venv/bin/python -m replay.recording_watch --queue "$queue" --defaults "$defaults" --jobs "$jobs")
case "${1:-}" in
  status) exec "${watching[@]}" --status ;;
  watch) exec "${watching[@]}" --until-event ;;
esac
[[ -f "$queue" ]] || echo '{"version":1,"slots":2,"paused":false,"recordings":[]}' > "$queue"
runner="queue-runner"
# Anchored: a tmux server's command line holds its first job's command too.
if pgrep -f "^.venv/bin/python -m replay.recording_queue run --queue $queue" >/dev/null; then
  echo "runner already ticking"
else
  [[ ! -e "$jobs/$runner.log" ]] || mv "$jobs/$runner.log" "$jobs/$runner-$(date +%Y%m%d%H%M%S).old"
  rm -rf "$jobs/$runner.status" "$jobs/$runner.launch"  # no runner holds them any more
  bash scripts/tmux-job.sh "$jobs" "$runner" \
    .venv/bin/python -m replay.recording_queue run --queue "$queue" --defaults "$defaults"
fi
# Its own tmux server: stopping the runner's server must not stop it.
if ! pgrep -f "^.venv/bin/python -m replay.disk_relief --apply --watch" >/dev/null; then
  mkdir -p edits/work/disk-relief
  bash scripts/tmux-job.sh edits/work/disk-relief "watch-$(date +%Y%m%d%H%M%S)" \
    .venv/bin/python -m replay.disk_relief --apply --watch
fi
bash scripts/tmux-job.sh "$jobs" "intake-$(date +%Y%m%d%H%M%S)" \
  .venv/bin/python -m replay.volume_resume --volume /Volumes/m.2-ssd -- \
  .venv/bin/python -m replay.youtube_intake --queue "$queue" \
  --root /Volumes/m.2-ssd/movie/youtube --ffmpeg-directory "$(brew --prefix ffmpeg-full)/bin" "$@"
echo "watch with: scripts/youtube-requests.sh watch"
