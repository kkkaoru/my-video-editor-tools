"""How long a displayed caption stays after its speech: readable, never stale, never late.

Paging ends a caption a fixed 80 ms after its last word, so a short reply vanished in a quarter
of a second (10/28: 49 % of the displayed captions were on screen under 1 s). Here the end of each
displayed caption (one Motion title: a chain of growing stages) is moved later, never earlier.
It is applied to the title's out point only (``motion_reveal_stages``), after its Type On keys
were computed from the unchanged stages, so the reveal and the cut exit stay exactly as they are;
the published SRT holds the same ends (``pipeline_completion.displayed``). A caption's required
on-screen time, from its first frame, is

    required = max(MIN_DISPLAY, characters / reading rate)

with the reading rate following the speech: ``clamp(characters / speech seconds, READING_CPS,
FAST_READING_CPS)``, so slow speech is read at the Japanese subtitle norm and fast speech, whose
text appears as fast as it is spoken, at most at the fast rate viewers still follow. The caption
also stays ``LINGER`` past its speech. The end is then bounded so that

- it never passes ``MAX_LINGER`` after its speech (no stale text over a silence) nor
  ``MAX_DISPLAY`` after its start;
- the next caption is never delayed: it ends ``MIN_GAP_FRAMES`` before the next caption starts
  (a cut exit has no transition to wait for), and a caption whose gap was already shorter keeps
  its end; a remaining gap under ``CHAIN_GAP`` is closed to ``MIN_GAP_FRAMES`` instead of flashing
  an empty frame, within the same bounds;
- it stays inside its part or the output (``limit``).

Sources:
- Netflix Japanese Timed Text Style Guide: up to 4 characters per second (``READING_CPS``).
- Netflix Timed Text Style Guide, subtitle timing: a minimum of 2 frames between subtitles; an
  out-time at least half a second past the end of the audio; gaps under half a second are chained
  to 2 frames, not extended past that half second (``MIN_GAP_FRAMES``, ``LINGER``, ``CHAIN_GAP``).
- Netflix Timed Text Style Guide, general requirements: minimum duration 5/6 s, maximum 7 s per
  event (``MIN_DISPLAY``, ``MAX_DISPLAY``).
- Szarkowska & Gerber-Moron (2018), "Viewers can keep up with fast subtitles: Evidence from eye
  movements", PLoS ONE 13(6): fast subtitles are followed without loss; Netflix's fastest CJK
  adult rate is 12 characters per second (Korean guide), the cap of ``FAST_READING_CPS``.
``MAX_LINGER`` equals ``MIN_DISPLAY``: a one-word reply spoken in an instant still reaches its
minimum duration, and nothing stays longer than that after its speech.
"""

from collections.abc import Sequence
from fractions import Fraction

READING_CPS = 4
FAST_READING_CPS = 12
MIN_DISPLAY = Fraction(5, 6)
MAX_DISPLAY = Fraction(7)
LINGER = Fraction(1, 2)
MAX_LINGER = MIN_DISPLAY
CHAIN_GAP = Fraction(1, 2)
MIN_GAP_FRAMES = 2


def characters(text: str) -> int:
    """Shown characters: line breaks and spaces are not read."""
    return len("".join(text.split()))


def required(text: str, start: Fraction, end: Fraction) -> Fraction:
    """On-screen time from the caption's start that its text needs at its speech's rate."""
    count = characters(text)
    spoken = Fraction(count) / (end - start)
    rate = min(max(spoken, Fraction(READING_CPS)), Fraction(FAST_READING_CPS))
    return max(MIN_DISPLAY, count / rate)


def held_end(
    text: str, start: Fraction, end: Fraction, *, following: Fraction, rate: int
) -> Fraction:
    """The caption's new end (never before ``end``); ``following`` is the next caption's start
    or the output's end."""
    latest = min(end + MAX_LINGER, start + MAX_DISPLAY, following - Fraction(MIN_GAP_FRAMES, rate))
    if latest <= end:
        return end
    wanted = min(max(start + required(text, start, end), end + LINGER), latest)
    if following - wanted < CHAIN_GAP:
        wanted = latest
    return max(end, wanted)


def held_frames(captions: Sequence[tuple[str, int, int]], *, rate: int, limit: int) -> list[int]:
    """The held end frame of each displayed caption (text, first frame, end frame), in order;
    ``limit`` is the frame no caption may reach (its part's or the output's end)."""
    ends: list[int] = []
    for index, (text, start, end) in enumerate(captions):
        following = captions[index + 1][1] if index + 1 < len(captions) else limit
        held = held_end(
            text, Fraction(start, rate), Fraction(end, rate), following=Fraction(following, rate),
            rate=rate,
        )  # fmt: skip
        ends.append(max(end, int(held * rate)))
    return ends
