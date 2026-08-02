use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const BASE_MIDI: i32 = 57;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Scale {
    #[default]
    MajorPentatonic,
    MinorPentatonic,
    Dorian,
    Mixolydian,
    Lydian,
    NaturalMinor,
}

impl Scale {
    pub fn intervals(&self) -> &'static [u8] {
        match self {
            Scale::MajorPentatonic => &[0, 2, 4, 7, 9],
            Scale::MinorPentatonic => &[0, 3, 5, 7, 10],
            Scale::Dorian => &[0, 2, 3, 5, 7, 9, 10],
            Scale::Mixolydian => &[0, 2, 4, 5, 7, 9, 10],
            Scale::Lydian => &[0, 2, 4, 6, 7, 9, 11],
            Scale::NaturalMinor => &[0, 2, 3, 5, 7, 8, 10],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Rhythm {
    #[default]
    Even,
    Tresillo,
    Gallop,
    Dotted,
    LongShort,
    Syncopated,
    Cascade,
}

impl Rhythm {
    pub fn pattern(&self) -> &'static [u8] {
        match self {
            Rhythm::Even => &[2, 2, 2, 2],
            Rhythm::Tresillo => &[3, 3, 2],
            Rhythm::Gallop => &[1, 1, 2, 2, 2],
            Rhythm::Dotted => &[3, 1, 3, 1],
            Rhythm::LongShort => &[4, 2, 2],
            Rhythm::Syncopated => &[2, 3, 3],
            Rhythm::Cascade => &[1, 1, 1, 1, 4],
        }
    }
}

/// The lead voice. All of them are smooth — no noise, no hard edges — so the
/// difference between two users is character rather than one of them sounding
/// rougher than the rest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Instrument {
    #[default]
    Synth,
    Soft,
    Bell,
    Chorus,
}

impl Instrument {
    pub fn timbre(self) -> Timbre {
        match self {
            Instrument::Synth => Timbre::Triangle,
            Instrument::Soft => Timbre::Soft,
            Instrument::Bell => Timbre::Bell,
            Instrument::Chorus => Timbre::Chorus,
        }
    }

    /// How much plain sine to double underneath, for the leads that want more
    /// body. `Soft` is already sine-based and only muddies itself.
    pub fn body(self) -> f32 {
        match self {
            Instrument::Synth => 0.10,
            Instrument::Soft => 0.0,
            Instrument::Bell => 0.08,
            Instrument::Chorus => 0.06,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct SonicSignature {
    pub root: u8,
    pub scale: Scale,
    pub progression: Vec<i8>,
    pub rhythm: Rhythm,
    pub instrument: Instrument,
}

const SCALES: [Scale; 6] = [
    Scale::MajorPentatonic,
    Scale::MinorPentatonic,
    Scale::Dorian,
    Scale::Mixolydian,
    Scale::Lydian,
    Scale::NaturalMinor,
];
const RHYTHMS: [Rhythm; 7] = [
    Rhythm::Even,
    Rhythm::Tresillo,
    Rhythm::Gallop,
    Rhythm::Dotted,
    Rhythm::LongShort,
    Rhythm::Syncopated,
    Rhythm::Cascade,
];
const INSTRUMENTS: [Instrument; 4] = [
    Instrument::Synth,
    Instrument::Soft,
    Instrument::Bell,
    Instrument::Chorus,
];
const PROGS: [&[i8]; 7] = [
    &[0, 4],
    &[0, 5],
    &[5, 0],
    &[0, 3],
    &[3, 4],
    &[0, 4, 5],
    &[0, 5, 3],
];

impl SonicSignature {
    pub fn from_string(s: &str) -> Self {
        let hash = Sha256::digest(s.as_bytes());

        Self {
            scale: SCALES[hash[0] as usize % SCALES.len()],
            root: hash[1] % 12,
            progression: PROGS[hash[2] as usize % PROGS.len()].to_vec(),
            rhythm: RHYTHMS[hash[6] as usize % RHYTHMS.len()],
            instrument: INSTRUMENTS[hash[7] as usize % INSTRUMENTS.len()],
        }
    }
}

pub const CUE_SECS: f32 = 1.4;

pub const RING_SECS: f32 = 2.8;

const CLICK_STRETCH: f32 = 2.5;

const MIN_NOTE: f32 = 0.04;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignatureEvent {
    Joined,
    Left,
    Ringtone,
    ProfileClick,
    Muted,
    Unmuted,
    Deafened,
    Undeafened,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BassMode {
    Full, // sustained root + a fifth swelling in under it
    Root, // sustained root only
    None,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Performance {
    /// How many notes the phrase walks across the ladder.
    pub notes: usize,
    /// Whether the walk generally climbs; `false` plays the same contour
    /// backwards, which is what makes opposite events exact mirrors.
    pub rising: bool,
    /// Onset of each repetition of the phrase, in seconds.
    pub cells: &'static [f32],
    /// Scale degrees each successive cell is lifted by — repeats that climb.
    pub lift: i32,
    /// Nominal seconds per note; `rhythm` redistributes time inside that span.
    pub step: f32,
    /// Note length as a fraction of its slot.
    pub hold: f32,
    /// Length of the phrase's final note — the part that rings out.
    pub tail: f32,
    /// Time multiplier; above 1.0 plays the same phrase slower.
    pub stretch: f32,
    pub octave_shift: i32,
    pub bass: BassMode,
    /// How loud the tonic is played — the bottom rung, the note the phrase is
    /// built around and resolves to. Under 1.0 it leaves a hole where the
    /// phrase would otherwise sit, which is what stops a mute from reading as
    /// a short leave; the levelling then brings the rest of the phrase up
    /// around the gap rather than just making the whole cue quieter.
    pub root_gain: f32,
    /// Loudness relative to a join. Applied by levelling the finished mix, so
    /// it holds regardless of which instrument the signature drew.
    pub gain: f32,
    /// Total render length. Everything is trimmed to fit inside it.
    pub window: f32,
}

impl Performance {
    pub fn content_end(&self) -> f32 {
        let last_cell = self.cells.last().copied().unwrap_or(0.0);
        let span = self.notes.max(1) as f32 * self.step;
        (last_cell + span + self.tail) * self.stretch
    }
}

impl SignatureEvent {
    /// Every event, in the order they're worth auditioning.
    pub const ALL: [SignatureEvent; 8] = [
        SignatureEvent::Joined,
        SignatureEvent::Left,
        SignatureEvent::Ringtone,
        SignatureEvent::ProfileClick,
        SignatureEvent::Muted,
        SignatureEvent::Unmuted,
        SignatureEvent::Deafened,
        SignatureEvent::Undeafened,
    ];

    pub fn label(self) -> &'static str {
        match self {
            SignatureEvent::Joined => "Join",
            SignatureEvent::Left => "Leave",
            SignatureEvent::Ringtone => "Ring",
            SignatureEvent::ProfileClick => "Click",
            SignatureEvent::Muted => "Mute",
            SignatureEvent::Unmuted => "Unmute",
            SignatureEvent::Deafened => "Deafen",
            SignatureEvent::Undeafened => "Undeafen",
        }
    }

    pub fn performance(self) -> Performance {
        match self {
            SignatureEvent::Joined => Performance {
                notes: 5,
                rising: true,
                cells: &[0.0],
                lift: 0,
                step: 0.075,
                hold: 0.9,
                tail: 0.3,
                stretch: 1.0,
                octave_shift: 0,
                bass: BassMode::Full,
                root_gain: 1.0,
                gain: 1.0,
                window: CUE_SECS,
            },
            SignatureEvent::Left => Performance {
                rising: false,
                ..SignatureEvent::Joined.performance()
            },
            SignatureEvent::Ringtone => Performance {
                notes: 4,
                rising: true,
                cells: &[0.0, 0.6, 1.2],
                lift: 1,
                step: 0.13,
                hold: 0.9,
                tail: 0.55,
                stretch: 1.0,
                octave_shift: 0,
                bass: BassMode::Root,
                root_gain: 1.0,
                gain: 0.77,
                window: RING_SECS,
            },
            SignatureEvent::ProfileClick => {
                let click = Performance {
                    cells: &[0.0, 0.55, 1.10],
                    stretch: CLICK_STRETCH,
                    gain: 0.1,
                    ..SignatureEvent::Ringtone.performance()
                };
                Performance {
                    window: click.content_end(),
                    ..click
                }
            }
            SignatureEvent::Muted => Performance {
                notes: 3,
                rising: false,
                cells: &[0.0],
                lift: 0,
                step: 0.07,
                hold: 0.85,
                tail: 0.26,
                stretch: 0.75,
                octave_shift: 0,
                bass: BassMode::None,
                root_gain: 0.3,
                gain: 0.49,
                window: CUE_SECS,
            },
            SignatureEvent::Unmuted => Performance {
                rising: true,
                ..SignatureEvent::Muted.performance()
            },
            SignatureEvent::Deafened => Performance {
                octave_shift: -12,
                ..SignatureEvent::Muted.performance()
            },
            SignatureEvent::Undeafened => Performance {
                rising: true,
                ..SignatureEvent::Deafened.performance()
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Timbre {
    Sine,     // bass / body
    Triangle, // hollow, odd harmonics
    Soft,     // sine with a little octave and fifth on top
    Bell,     // two-operator FM whose brightness falls away
    Chorus,   // two triangles a few cents apart
}

/// A single scheduled note — everything the player needs, no theory required.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct NoteEvent {
    pub freq: f32,  // Hz
    pub start: f32, // seconds from playback start
    pub dur: f32,
    pub gain: f32, // 0..1
    pub pan: f32,  // -1..1
    pub attack: f32,
    pub release: f32,
    pub timbre: Timbre,
}

fn midi_to_freq(m: i32) -> f32 {
    440.0 * 2f32.powf((m as f32 - 69.0) / 12.0)
}

/// Resolve a scale degree (which may span octaves) to a MIDI note.
fn scale_note(intervals: &[u8], base: i32, deg: i32) -> i32 {
    let n = intervals.len() as i32;
    let oct = deg.div_euclid(n);
    let idx = deg.rem_euclid(n) as usize;
    base + 12 * oct + intervals[idx] as i32
}

#[allow(clippy::too_many_arguments)]
fn note(
    freq: f32,
    start: f32,
    dur: f32,
    gain: f32,
    pan: f32,
    attack: f32,
    release: f32,
    timbre: Timbre,
) -> NoteEvent {
    NoteEvent {
        freq,
        start,
        dur,
        gain,
        pan,
        attack,
        release,
        timbre,
    }
}

/// Nothing may run past the window. A note that starts too late to breathe is
/// dropped; one that merely overruns is trimmed (its release scales with its
/// length, so a trimmed note still fades out instead of clicking).
fn fit(window: f32, start: f32, dur: f32) -> Option<f32> {
    let room = window - start;
    (room >= MIN_NOTE).then(|| dur.min(room))
}

/// Rungs in the ladder every phrase is drawn from.
const LADDER: usize = 6;

impl SonicSignature {
    /// One well-mixed number off every field of the signature. Drives the
    /// choices that make two users' phrases differ in shape, not just in pitch.
    fn seed(&self) -> u32 {
        let mut h: u32 = 2_166_136_261; // FNV-1a
        let mut mix = |b: u8| {
            h ^= b as u32;
            h = h.wrapping_mul(16_777_619);
        };
        mix(self.root);
        mix(self.scale as u8);
        mix(self.rhythm as u8);
        mix(self.instrument as u8);
        for &d in &self.progression {
            mix(d as u8);
        }
        h
    }

    /// The rungs every phrase draws from: six scale degrees, lowest to highest.
    /// The gaps between them — one scale step or two — are the signature's own,
    /// so two users climb the same distance by a different route.
    pub fn ladder(&self) -> [i32; LADDER] {
        let seed = self.seed();
        let mut rungs = [0_i32; LADDER];
        for i in 1..LADDER {
            rungs[i] = rungs[i - 1] + 1 + ((seed >> i) & 1) as i32;
        }
        rungs
    }

    /// A generally-climbing walk across the ladder: `notes` rungs spread from
    /// the bottom to the top, then one or two of the steps turned back on
    /// themselves so the line isn't a plain scale run. How many steps double
    /// back and where is the signature's own — the shape is part of the
    /// identity, not just the pitches.
    pub fn contour(&self, notes: usize) -> Vec<usize> {
        if notes <= 1 {
            return vec![0];
        }
        let top = LADDER - 1;
        let mut walk: Vec<usize> = (0..notes).map(|k| k * top / (notes - 1)).collect();

        // Swapping the rungs either side of a step is what turns that step
        // downward; its neighbours still climb, so the phrase keeps its arc.
        // The first and last steps are left alone — they set the arc.
        let slots = notes.saturating_sub(2);
        if slots == 0 {
            return walk;
        }
        let seed = self.seed() as usize;
        let turns = ((seed >> 6) % 3).min(slots);
        let mut turned: Vec<usize> = Vec::with_capacity(turns);
        for t in 0..turns {
            let p = 1 + (seed >> (9 + 5 * t)) % slots;
            // two swaps sharing a rung would partly undo each other
            if turned.iter().any(|&q: &usize| p.abs_diff(q) < 2) {
                continue;
            }
            walk.swap(p, p + 1);
            turned.push(p);
        }
        walk
    }

    pub fn schedule(&self, event: SignatureEvent) -> Vec<NoteEvent> {
        let perf = event.performance();
        let ladder = self.ladder();
        let mut walk = self.contour(perf.notes);
        if !perf.rising {
            walk.reverse();
        }
        let intervals = self.scale.intervals();
        let base = BASE_MIDI + self.root as i32 + perf.octave_shift;
        let lead_timbre = self.instrument.timbre();
        let body = self.instrument.body();
        let attack = 0.006 * perf.stretch;

        // The rhythm spaces the notes unevenly, but normalised against its own
        // total, so a phrase spans the same time whichever pattern was drawn.
        let pat = self.rhythm.pattern();
        let slots: Vec<f32> = (0..walk.len()).map(|k| pat[k % pat.len()] as f32).collect();
        let weight: f32 = slots.iter().sum();
        let span = walk.len() as f32 * perf.step;

        let mut out: Vec<NoteEvent> = Vec::new();
        for (ci, &cell) in perf.cells.iter().enumerate() {
            let cell_start = cell * perf.stretch;
            let final_cell = ci + 1 == perf.cells.len();
            let lift = perf.lift * ci as i32; // successive repeats climb
            let mut cell_end = cell_start;
            let mut offset = 0.0_f32;

            // --- LEAD ---
            for (k, &rung) in walk.iter().enumerate() {
                let slot = slots[k] / weight * span;
                let start = cell_start + offset * perf.stretch;
                offset += slot;

                // the phrase's very last note is the one that rings out
                let want = if final_cell && k + 1 == walk.len() {
                    perf.tail
                } else {
                    slot * perf.hold
                };
                let Some(dur) = fit(perf.window, start, want * perf.stretch) else {
                    continue;
                };
                // the bottom rung is the tonic; some events play it under
                // strength, or not at all, to leave a hole where it would land
                let voice = if rung == 0 { perf.root_gain } else { 1.0 };
                if voice <= 0.0 {
                    continue;
                }
                cell_end = cell_end.max(start + dur);

                let m = scale_note(intervals, base, ladder[rung] + lift);
                let pan = if k % 2 == 0 { -0.14 } else { 0.14 };
                out.push(note(
                    midi_to_freq(m),
                    start,
                    dur,
                    0.34 * voice,
                    pan,
                    attack,
                    dur * 0.55,
                    lead_timbre,
                ));
                if body > 0.0 {
                    // soft sine body under the lead
                    out.push(note(
                        midi_to_freq(m),
                        start,
                        dur,
                        body * voice,
                        0.0,
                        attack,
                        dur * 0.55,
                        Timbre::Sine,
                    ));
                }
            }

            // --- BASS --- one sustain per cell, sized to what the cell played.
            // An octave under the motif, not two: low enough to be foundation,
            // high enough that a laptop speaker still reproduces it.
            if perf.bass != BassMode::None && cell_end > cell_start {
                let root = scale_note(intervals, base, ladder[0] + lift) - 12;
                let len = cell_end - cell_start;
                out.push(note(
                    midi_to_freq(root),
                    cell_start,
                    len,
                    0.32,
                    0.0,
                    0.02,
                    len * 0.45,
                    Timbre::Sine,
                ));
                if perf.bass == BassMode::Full {
                    let swell = cell_start + len * 0.35;
                    if let Some(dur) = fit(perf.window, swell, len * 0.65) {
                        out.push(note(
                            midi_to_freq(root + 7),
                            swell,
                            dur,
                            0.17,
                            0.0,
                            dur * 0.3,
                            dur * 0.6,
                            Timbre::Sine,
                        ));
                    }
                }
            }
        }

        out
    }
}

const SAMPLE_RATE: u32 = 44_100;

// The thing for an `<audio>` tag: `data:audio/wav;base64,...`
pub fn signature_audio_src(sig: &SonicSignature, event: SignatureEvent) -> String {
    let wav = render_wav(sig, event);
    format!("data:audio/wav;base64,{}", STANDARD.encode(&wav))
}

/// Render the whole schedule to a 16-bit stereo WAV byte vector. Always exactly
/// the event's window long — the schedule is trimmed to fit, never the reverse.
pub fn render_wav(sig: &SonicSignature, event: SignatureEvent) -> Vec<u8> {
    let perf = event.performance();
    let notes = sig.schedule(event);
    let frames = (perf.window * SAMPLE_RATE as f32).ceil() as usize;

    let mut left = vec![0.0_f32; frames];
    let mut right = vec![0.0_f32; frames];
    for n in &notes {
        mix_note(n, &mut left, &mut right);
    }
    let sounding = notes
        .iter()
        .map(|n| n.start + n.dur)
        .fold(0.0_f32, f32::max);
    normalise(&mut left, &mut right, perf.gain, sounding);
    encode_wav_stereo(&left, &right)
}

/// How far the chorus voice's partner is detuned — about ten cents, enough to
/// shimmer and not enough to sour.
const DETUNE: f32 = 1.006;

fn sine(phase: f32) -> f32 {
    (phase * std::f32::consts::TAU).sin()
}

fn triangle(phase: f32) -> f32 {
    4.0 * (phase - (phase + 0.5).floor()).abs() - 1.0
}

/// One sample of a voice. `partner` is a second phase running slightly sharp,
/// and `progress` is how far through the note we are — that's what lets the
/// bell lose its brightness as it rings.
fn wave(timbre: Timbre, phase: f32, partner: f32, progress: f32) -> f32 {
    match timbre {
        Timbre::Sine => sine(phase),
        Timbre::Triangle => triangle(phase),
        Timbre::Soft => 0.72 * sine(phase) + 0.22 * sine(phase * 2.0) + 0.06 * sine(phase * 3.0),
        Timbre::Bell => {
            // index falls to nothing, so the note settles into a pure sine
            let index = 1.8 * (-3.0 * progress).exp();
            (phase * std::f32::consts::TAU + index * sine(phase * 2.0)).sin()
        }
        Timbre::Chorus => 0.5 * (triangle(phase) + triangle(partner)),
    }
}

fn mix_note(n: &NoteEvent, left: &mut [f32], right: &mut [f32]) {
    let sr = SAMPLE_RATE as f32;
    let start = (n.start * sr) as usize;
    let count = (n.dur * sr).ceil() as usize;
    // equal-power pan
    let lg = (((1.0 - n.pan) * 0.5).max(0.0)).sqrt();
    let rg = (((1.0 + n.pan) * 0.5).max(0.0)).sqrt();

    let dphase = n.freq / sr;
    let dpartner = n.freq * DETUNE / sr;
    let mut phase = 0.0_f32;
    // a quarter cycle apart to start with: dead in phase, the chorus pair
    // would begin at full amplitude and then drift into cancelling itself
    let mut partner = 0.25_f32;
    for i in 0..count {
        let idx = start + i;
        if idx >= left.len() {
            break;
        }
        let t = i as f32 / sr;
        let amp = envelope(t, n) * n.gain;
        let s = wave(n.timbre, phase, partner, t / n.dur) * amp;
        left[idx] += s * lg;
        right[idx] += s * rg;
        phase = (phase + dphase).fract();
        partner = (partner + dpartner).fract();
    }
}

/// Linear attack / release envelope (1.0 in the sustain region).
fn envelope(t: f32, n: &NoteEvent) -> f32 {
    if t < n.attack {
        (t / n.attack).clamp(0.0, 1.0)
    } else if t > n.dur - n.release {
        ((n.dur - t) / n.release).clamp(0.0, 1.0)
    } else {
        1.0
    }
}

/// What a join levels out to. Everything else is a fraction of it.
const TARGET_RMS: f32 = 0.18;

/// Every voice has a different RMS for the same amplitude — a bell is a
/// full-swing phase-modulated sine, a triangle is not, and `Soft`'s partials
/// don't peak together — so otherwise two users' cues differ by several dB
/// purely by which instrument their signature drew. Levelling the mix rather
/// than the waveform also absorbs the body layer and the note count, and keeps
/// working if a voice is ever retuned.
fn normalise(left: &mut [f32], right: &mut [f32], gain: f32, sounding: f32) {
    // measured over the sounding span only: trailing silence differs per event,
    // and counting it would drag the sparser events louder to compensate
    let frames = ((sounding * SAMPLE_RATE as f32) as usize).min(left.len());
    if frames > 0 {
        let energy: f64 = left[..frames]
            .iter()
            .chain(right[..frames].iter())
            .map(|&x| (x * x) as f64)
            .sum();
        let rms = (energy / (frames * 2) as f64).sqrt() as f32;
        if rms > 0.0 {
            let g = TARGET_RMS * gain / rms;
            for x in left.iter_mut().chain(right.iter_mut()) {
                *x *= g;
            }
        }
    }

    // and only then keep it inside the rails
    let peak = left
        .iter()
        .chain(right.iter())
        .fold(0.0_f32, |m, &x| m.max(x.abs()));
    if peak > 0.9 {
        let g = 0.9 / peak;
        for x in left.iter_mut().chain(right.iter_mut()) {
            *x *= g;
        }
    }
}

fn encode_wav_stereo(left: &[f32], right: &[f32]) -> Vec<u8> {
    let frames = left.len();
    let channels: u16 = 2;
    let bits: u16 = 16;
    let block_align = channels * (bits / 8);
    let byte_rate = SAMPLE_RATE * block_align as u32;
    let data_len = (frames * block_align as usize) as u32;

    let mut buf = Vec::with_capacity(44 + data_len as usize);
    buf.extend_from_slice(b"RIFF");
    buf.extend_from_slice(&(36 + data_len).to_le_bytes());
    buf.extend_from_slice(b"WAVE");
    buf.extend_from_slice(b"fmt ");
    buf.extend_from_slice(&16u32.to_le_bytes()); // PCM fmt chunk size
    buf.extend_from_slice(&1u16.to_le_bytes()); // format = PCM
    buf.extend_from_slice(&channels.to_le_bytes());
    buf.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    buf.extend_from_slice(&byte_rate.to_le_bytes());
    buf.extend_from_slice(&block_align.to_le_bytes());
    buf.extend_from_slice(&bits.to_le_bytes());
    buf.extend_from_slice(b"data");
    buf.extend_from_slice(&data_len.to_le_bytes());
    for i in 0..frames {
        for &sample in &[left[i], right[i]] {
            let v = (sample.clamp(-1.0, 1.0) * 32767.0) as i16;
            buf.extend_from_slice(&v.to_le_bytes());
        }
    }
    buf
}
