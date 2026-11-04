"""The PTS lines of FFmpeg's ``showinfo`` filter in a decoder's log.

The decoder can print a warning without a newline right before a ``showinfo`` line: an H.264
``Late SEI`` made ``Late SEIn:116962 pts:59884544`` (youtube-1GUiocLmiFw), which a word boundary
before ``n:`` missed, so a complete decode looked one frame short of its clock. Only a lower-case
letter, digit or underscore before ``n:`` means another word.
"""

import re

SHOWN_PTS = re.compile(rb"(?<![a-z0-9_])n:\s*\d+\s+pts:\s*(-?\d+)\s")
