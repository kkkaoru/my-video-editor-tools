// Resume playback where the viewer left off: one position per catalog video and source
// ("edited" / "original"), kept in this browser's localStorage. Loaded by index.html with
// import(); the pure rules are unit-tested in src/resume.test.ts.

// Positions this close to the start are not worth resuming.
export const RESUME_MIN_SECONDS = 5;
// Positions this close to the end mean the video was finished: start over.
export const RESUME_END_MARGIN_SECONDS = 10;
// timeupdate fires ~4 times a second; write at most this often while playing.
export const RESUME_WRITE_INTERVAL_MS = 5000;
// HTMLMediaElement.HAVE_METADATA: before it, currentTime belongs to no loaded movie yet.
const HAVE_METADATA = 1;
const KEY_PREFIX = "resume:";

export const resumeKey = (id, source) => `${KEY_PREFIX}${id}:${source}`;

// Where to start from a saved position: 0 unless it is past the start and short of the end.
export function resumeStart(saved, duration) {
  if (!Number.isFinite(saved) || saved < RESUME_MIN_SECONDS) return 0;
  if (!Number.isFinite(duration) || duration <= 0) return saved;
  return saved > duration - RESUME_END_MARGIN_SECONDS ? 0 : saved;
}

export const writeDue = (now, lastWrite) => now - lastWrite >= RESUME_WRITE_INTERVAL_MS;

// Storage may be missing or throw (private mode, blocked site data): playback goes on without it.
export function readPosition(storage, key) {
  try {
    return Number.parseFloat(storage().getItem(key) ?? "");
  } catch (error) {
    console.warn(`resume: reading ${key} failed: ${error}`);
    return Number.NaN;
  }
}

export function writePosition(storage, key, seconds) {
  try {
    storage().setItem(key, seconds.toFixed(1));
    return true;
  } catch (error) {
    console.warn(`resume: writing ${key} failed: ${error}`);
    return false;
  }
}

// The key is set next to every src assignment, so events of the outgoing movie never land on
// the incoming one's key, and nothing is written before the new movie's metadata is loaded.
const savable = (video) => Boolean(video.dataset.resumeKey) && video.readyState >= HAVE_METADATA;

// Seeks a freshly loaded movie to its saved position unless something already chose a time
// (an explicit seek, a search hit or a clip).
export function restorePosition(video, storage) {
  if (!video.dataset.resumeKey || video.seeking || video.currentTime > 0) return;
  const start = resumeStart(readPosition(storage, video.dataset.resumeKey), video.duration);
  if (start > 0) video.currentTime = start;
}

// Saves the position: throttled while playing (and while clips loop through seeks), at once on
// pause, end and when the page is hidden or left. A failed write is throttled too, so blocked
// storage warns once per interval rather than on every timeupdate.
export function trackPositions({ video, page, document, storage, now }) {
  const last = { at: Number.NEGATIVE_INFINITY };
  const save = () => {
    if (!savable(video)) return;
    writePosition(storage, video.dataset.resumeKey, video.currentTime);
    last.at = now();
  };
  const throttled = () => {
    if (writeDue(now(), last.at)) save();
  };
  video.addEventListener("timeupdate", throttled);
  video.addEventListener("seeked", throttled);
  video.addEventListener("pause", save);
  video.addEventListener("ended", save);
  page.addEventListener("pagehide", save);
  document.addEventListener("visibilitychange", () => {
    if (document.visibilityState === "hidden") save();
  });
  return save;
}
