// Playback times for the viewer page: the one formatter every time label uses (H:MM:SS.mmm) and
// the millisecond clock beside the player, which the native controls cannot show. Loaded by
// index.html with import() before the first render; unit-tested in src/playtime.test.ts.

const MS_PER_SECOND = 1000;
const MS_PER_MINUTE = 60_000;
const MS_PER_HOUR = 3_600_000;
const UNKNOWN_DURATION = "-:--:--.---";

const pad = (value, width) => String(value).padStart(width, "0");

// 3723.456 → "1:02:03.456"; negative or unknown times read as 0.
export function formatTime(seconds) {
  const ms = Number.isFinite(seconds) ? Math.round(Math.max(0, seconds) * MS_PER_SECOND) : 0;
  const hours = Math.floor(ms / MS_PER_HOUR);
  const minutes = Math.floor((ms % MS_PER_HOUR) / MS_PER_MINUTE);
  return `${hours}:${pad(minutes, 2)}:${pad(Math.floor((ms % MS_PER_MINUTE) / MS_PER_SECOND), 2)}.${pad(ms % MS_PER_SECOND, 3)}`;
}

export const clockText = (current, duration) =>
  `${formatTime(current)} / ${Number.isFinite(duration) && duration > 0 ? formatTime(duration) : UNKNOWN_DURATION}`;

// Redraws the clock every animation frame while the movie plays and the page is visible
// (timeupdate alone is ~4 Hz); otherwise only when the position or movie changes.
export function trackClock({ video, element, document, frame, cancel }) {
  const loop = { id: 0 };
  const render = () => {
    element.textContent = clockText(video.currentTime, video.duration);
  };
  const tick = () => {
    render();
    loop.id = frame(tick);
  };
  const stop = () => {
    if (loop.id !== 0) cancel(loop.id);
    loop.id = 0;
    render();
  };
  const start = () => {
    if (loop.id === 0 && !video.paused && document.visibilityState !== "hidden") loop.id = frame(tick);
  };
  video.addEventListener("playing", start);
  video.addEventListener("pause", stop);
  video.addEventListener("ended", stop);
  video.addEventListener("emptied", stop);
  for (const name of ["seeked", "timeupdate", "loadedmetadata", "durationchange"]) video.addEventListener(name, render);
  document.addEventListener("visibilitychange", () => (document.visibilityState === "hidden" ? stop() : start()));
  render();
}
