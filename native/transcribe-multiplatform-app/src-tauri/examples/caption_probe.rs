//! Throwaway.
use std::path::{Path, PathBuf};
use transcribe_app::captions::build_cues;
use transcribe_app::export::Transcript;
use transcribe_app::morphology::Segmenter;
use transcribe_app::settings::{Anchor, Settings};
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let root = PathBuf::from(&args[1]);
    let seg = Segmenter::new(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/ipadic-mecab-2_7_0.dic.zst"),
    );
    for f in args[2].split(',') {
        let t = Transcript::load(&root.join(format!("{f}.transcript.json"))).unwrap();
        for anchor in [Anchor::FaceAbove, Anchor::FaceBelow, Anchor::VideoBottom] {
            let mut s = Settings::default();
            s.placement.anchor = anchor;
            let (cues, _) = build_cues(&t.words, &t.faces, &t.info, &s, &seg).unwrap();
            let m = transcribe_app::captions::metrics_for(&t.info, &s);
            let box_h = |n: usize| {
                m.font_px * (150.0 / 82.955_801_104_972_38 + (n as f64 - 1.0) * s.line_height)
            };
            let wid = |c: &transcribe_app::captions::Cue| {
                c.lines
                    .iter()
                    .map(|l| {
                        l.chars()
                            .map(|ch| if ch.is_ascii() { 0.6 } else { 1.0 } * m.font_px)
                            .sum::<f64>()
                    })
                    .fold(0.0, f64::max)
                    + 2.0 * (m.outline_px + s.rim_ratio * m.font_px)
            };
            let mut overlap = 0;
            let mut overlap_samples = 0;
            for c in &cues {
                let (w, h) = (wid(c), box_h(c.lines.len()));
                let hits = t
                    .faces
                    .iter()
                    .filter(|fa| fa.time >= c.start && fa.time <= c.end)
                    .filter(|fa| {
                        let fh = fa.bottom - fa.top;
                        c.y - h / 2.0 < fa.bottom
                            && c.y + h / 2.0 > fa.top
                            && c.x - w / 2.0 < fa.center_x + fh / 2.0
                            && c.x + w / 2.0 > fa.center_x - fh / 2.0
                    })
                    .count();
                if hits > 0 {
                    overlap += 1;
                    overlap_samples += hits;
                }
            }
            let chars = |c: &transcribe_app::captions::Cue| {
                c.lines
                    .iter()
                    .map(|l| l.chars().filter(|x| !x.is_whitespace()).count())
                    .sum::<usize>()
            };
            let mut ys: Vec<f64> = cues.iter().map(|c| c.y).collect();
            let jumps = ys
                .windows(2)
                .filter(|w| (w[0] - w[1]).abs() > m.font_px * 2.0)
                .count();
            ys.sort_by(f64::total_cmp);
            let short: Vec<_> = cues
                .iter()
                .filter(|c| c.end - c.start < 5.0 / 6.0 - 1e-6)
                .collect();
            println!("{f} {anchor:?}: cues={} overlapCues={overlap} overlapSamples={overlap_samples} yJumps={jumps} y[p5,50,95]=[{:.0},{:.0},{:.0}] maxLineChars={} maxPage={} under5/6={} over7={} cps>12={} overlapTime={} ",
                cues.len(), ys[ys.len()/20], ys[ys.len()/2], ys[ys.len()*19/20],
                cues.iter().flat_map(|c| c.lines.iter()).filter(|l| !l.is_ascii()).map(|l| l.chars().count()).max().unwrap(), cues.iter().filter(|c| !c.lines.concat().is_ascii()).map(chars).max().unwrap(),
                short.len(), cues.iter().filter(|c| c.end - c.start > 7.0 + 1e-6).count(),
                cues.iter().filter(|c| !c.lines.concat().is_ascii() && chars(c) as f64 / (c.end-c.start) > 12.0).count(),
                cues.windows(2).filter(|w| w[0].end > w[1].start).count());
            if anchor == Anchor::FaceAbove {
                for w in cues
                    .windows(2)
                    .filter(|w| {
                        !w[0].lines.concat().is_ascii()
                            && chars(&w[0]) as f64 / (w[0].end - w[0].start) > 12.0
                    })
                    .take(6)
                {
                    println!(
                        "   fast {:.2}-{:.2} next {:.2} {}",
                        w[0].start,
                        w[0].end,
                        w[1].start,
                        w[0].lines.join("/")
                    );
                }
                for c in &cues {
                    let (wi, hi) = (wid(c), box_h(c.lines.len()));
                    let bad = t
                        .faces
                        .iter()
                        .filter(|fa| fa.time >= c.start && fa.time <= c.end)
                        .find(|fa| {
                            let fh = fa.bottom - fa.top;
                            c.y - hi / 2.0 < fa.bottom
                                && c.y + hi / 2.0 > fa.top
                                && c.x - wi / 2.0 < fa.center_x + fh / 2.0
                                && c.x + wi / 2.0 > fa.center_x - fh / 2.0
                        });
                    if let Some(fa) = bad {
                        println!(
                            "   hit {:.1} {} y{:.0} x{:.0} w{:.0} face {:.0}-{:.0} cx{:.0}",
                            c.start,
                            c.lines.join("/"),
                            c.y,
                            c.x,
                            wi,
                            fa.top,
                            fa.bottom,
                            fa.center_x
                        );
                    }
                }
                for c in short.iter().take(14) {
                    println!(
                        "   short {:.2}-{:.2} {:?} {}",
                        c.start,
                        c.end,
                        c.speaker,
                        c.lines.join("/")
                    );
                }
            }
        }
    }
}
