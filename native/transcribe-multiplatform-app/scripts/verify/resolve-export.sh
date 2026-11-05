#!/usr/bin/env bash
# Development only, never shipped: imports an FCPXML into DaVinci Resolve and writes a YouTube-encoded MP4 from the Deliver page.
# usage: resolve-export.sh <absolute .fcpxml> <absolute output.mp4> [--keep]
# It sends input only after 15 s without keyboard or mouse use, starts only when Resolve is closed,
# and removes its temporary project afterwards unless --keep is given. It never uploads anything.
set -euo pipefail
fcpxml="${1:?fcpxml}"; output="${2:?output.mp4}"; keep="${3:-}"
case "$fcpxml$output" in /*/*) ;; *) echo "paths must be absolute" >&2; exit 2 ;; esac
[[ "$output" == *.mp4 ]] || { echo "the output must be .mp4" >&2; exit 2; }
[ ! -e "$output" ] || { echo "the output exists: $output" >&2; exit 2; }
root="$(cd "$(dirname "$0")/../.." && pwd -P)"
app_process="DaVinci Resolve.app/Contents/MacOS/Resolve"
projects="$HOME/Library/Application Support/Blackmagic Design/DaVinci Resolve/Resolve Project Library/Resolve Projects/Users/guest/Projects"
project="verify-export-$$"
tools="$root/node_modules/.cache/resolve-export"
out_dir="$(dirname "$output")"; name="$(basename "$output" .mp4)"
# Screen points of Resolve's maximised window; check_window refuses any other window geometry.
window_geometry="0, 39, 1800, 1130"
preset_box=(225 132); youtube_row=(195 353); file_name_box=(223 186)
youtube_x=390

pgrep -f "$app_process" >/dev/null && { echo "close DaVinci Resolve first" >&2; exit 2; }
media="$(grep -o 'src="file://[^"]*"' "$fcpxml" | head -1 | sed -E 's/^src="file:\/\/(.*)"$/\1/; s/%20/ /g')"
[ -f "$media" ] || { echo "the video in the FCPXML is missing: $media" >&2; exit 2; }
# Resolve's files carry a stream group, which adds lines: take the first video line and the format's duration apart.
video_size() { ffprobe -v error -select_streams v:0 -show_entries stream=width,height -of csv=p=0 "$1" | grep -m1 '^[0-9]*,[0-9]*' | cut -d, -f1,2 | tr ',' ' '; }
length_of() { ffprobe -v error -show_entries format=duration -of csv=p=0 "$1"; }
read -r width height <<<"$(video_size "$media")"
# The timeline's length from the FCPXML: a timeline may use only part of its video.
duration="$(grep -o '<sequence [^>]*' "$fcpxml" | head -1 | grep -o 'duration="[^"]*"' | sed -E 's/duration="([0-9]+)(\/([0-9]+))?s"/\1 \3/' | awk '{ print $1 / ($2 == "" ? 1 : $2) }')"
[ -n "$duration" ] || { echo "no timeline length in the FCPXML" >&2; exit 2; }
short_edge=$(( width < height ? width : height ))
case "$short_edge" in
  720) youtube_y=345 ;;
  1080) youtube_y=365 ;;
  1440) youtube_y=385 ;;
  2160) youtube_y=405 ;;
  *) echo "no YouTube preset for a short edge of $short_edge" >&2; exit 2 ;;
esac

mkdir -p "$tools" "$out_dir"
for helper in gui-idle gui-axpress gui-click; do
  [ "$tools/$helper" -nt "$root/scripts/verify/$helper.swift" ] || swiftc -O "$root/scripts/verify/$helper.swift" -o "$tools/$helper"
done

front() {
  "$tools/gui-idle" 15 900 >/dev/null || { echo "the Mac stayed in use; nothing sent" >&2; exit 3; }
  osascript -e 'tell application "System Events" to tell process "Resolve" to set frontmost to true'
  sleep 0.5
  [ "$(osascript -e 'tell application "System Events" to get name of first process whose frontmost is true')" = "Resolve" ] \
    || { echo "Resolve is not in front; nothing sent" >&2; exit 3; }
}
windows() { osascript -e 'tell application "System Events" to tell process "Resolve" to get name of windows' 2>/dev/null; }
wait_window() {
  for _ in $(seq 1 "$2"); do windows | grep -q "$1" && return 0; sleep 1; done
  echo "no window named $1: $(windows)" >&2; exit 4
}
press() { front; "$tools/gui-axpress" "$(pgrep -f "$app_process")" "$1" >/dev/null 2>&1 || true; }
click() { front; "$tools/gui-click" "$1" "$2"; }
system_events() { front; osascript -e "tell application \"System Events\" to tell process \"Resolve\" to $1" >/dev/null; }
go_to_path() {
  front
  osascript <<SCRIPT >/dev/null
tell application "System Events" to tell process "Resolve"
  keystroke "g" using {command down, shift down}
  delay 1.5
  key code 102
  delay 0.2
  keystroke "$1"
  delay 1.5
  key code 36
  delay 2.5
  key code 36
end tell
SCRIPT
}
window_bounds() {
  osascript - "$project" <<'SCRIPT'
on run argv
  tell application "System Events" to tell process "Resolve"
    set w to window (item 1 of argv)
    return {item 1 of (get position of w), item 2 of (get position of w), item 1 of (get size of w), item 2 of (get size of w)}
  end tell
end run
SCRIPT
}
check_window() {
  [ "$(window_bounds)" = "$window_geometry" ] || { echo "the Resolve window is not maximised; the clicks would miss" >&2; exit 4; }
}
finish() {
  pkill -f "$app_process" || true
  sleep 5
  [ "$keep" = "--keep" ] || rm -rf "${projects:?}/${project:?}"
}
trap finish EXIT

echo "step: new project $project"
open -a "DaVinci Resolve"
wait_window "Project Manager" 180
sleep 6
press "New Project"
wait_window "Create New Project" 20
system_events "set value of text field 1 of window \"Create New Project\" to \"$project\""
sleep 1
system_events "click button \"Create\" of window \"Create New Project\""
wait_window "$project" 90
sleep 15
check_window

echo "step: import $fcpxml"
system_events 'click menu item "Timeline…" of menu 1 of menu item "Import" of menu "File" of menu bar item "File" of menu bar 1'
sleep 2.5
go_to_path "$fcpxml"
wait_window "Import a Timeline" 60
sleep 2
system_events 'click button "Ok" of window "Import a Timeline From File"'
sleep 4
windows | grep -q "Message" && { echo "Resolve could not find the video" >&2; exit 4; }
for _ in $(seq 1 180); do windows | grep -q "Import a Timeline" || break; sleep 2; done

echo "step: Deliver page, YouTube preset for a short edge of $short_edge"
system_events 'click menu item "Deliver" of menu 1 of menu item "Switch to Page" of menu "Workspace" of menu bar item "Workspace" of menu bar 1'
sleep 6
click "${preset_box[@]}"
sleep 2
click "${youtube_row[@]}"
sleep 2
click "$youtube_x" "$youtube_y"
sleep 3

echo "step: file name and folder"
click "${file_name_box[@]}"
sleep 1
front
osascript <<SCRIPT >/dev/null
tell application "System Events"
  keystroke "a" using {command down}
  delay 0.3
  keystroke "$name"
  delay 0.3
  key code 36
end tell
SCRIPT
sleep 1
press "Browse"
wait_window "File Destination" 20
sleep 2
go_to_path "$out_dir"
sleep 3

echo "step: render"
press "Add to Render Queue"
sleep 3
press "Render All"
for _ in $(seq 1 60); do [ -f "$output" ] && break; sleep 2; done
[ -f "$output" ] || { echo "the render did not start" >&2; exit 4; }
last=-1
for _ in $(seq 1 2880); do
  size="$(stat -f %z "$output")"
  [ "$size" = "$last" ] && ! lsof "$output" >/dev/null 2>&1 && break
  last="$size"; sleep 5
done

echo "step: check the file"
read -r format <<<"$(ffprobe -v error -show_entries format=format_name -of csv=p=0 "$output")"
read -r out_width out_height <<<"$(video_size "$output")"
out_duration="$(length_of "$output")"
[[ "$format" == *mp4* ]] || { echo "not an mp4: $format" >&2; exit 5; }
[ "$out_width" = "$width" ] && [ "$out_height" = "$height" ] || { echo "the size changed: ${out_width}x$out_height" >&2; exit 5; }
awk -v a="$duration" -v b="$out_duration" 'BEGIN { d = a - b; exit (d < 1 && d > -1) ? 0 : 1 }' || { echo "the length changed: $out_duration" >&2; exit 5; }
echo "done: $output ${out_width}x$out_height ${out_duration}s"
