//! Live waveform, pill motion and recording controls.

use super::*;

// Waveform constants.
pub(super) const BARS: usize = 10;
// Whole points, so every bar edge lands on a pixel on 1x displays.
const BAR_W: f32 = 2.;
const BAR_GAP: f32 = 3.;
const BAR_MIN: f32 = 4.;
const BAR_MAX: f32 = 18.;
const ENVELOPE: [f32; BARS] = [0.35, 0.55, 0.75, 0.92, 1., 1., 0.92, 0.75, 0.55, 0.35];
/// Input levels mapped onto the bars: the quiet end of a laptop mic's
/// speech range shows, and close speech fills them.
const LEVEL_FLOOR_DB: f32 = -55.;
const LEVEL_CEIL_DB: f32 = -20.;
/// Peak height of the idle breath while no speech is heard. It follows
/// [`ENVELOPE`], so silence reads as a soft arch, not a flat row of dots.
const BREATH: f32 = 0.38;
const RISE: f32 = 0.45;
const FALL: f32 = 0.16;
/// The record dot that says the microphone is live: the pill's only state
/// color, so it is sized to hold its own on a busy desktop.
const REC_DOT: f32 = 8.;
const REC_GAP: f32 = 7.;
/// The three-dot motif that replaces the bars while transcribing.
const THINK_DOTS: usize = 3;
const THINK_DOT: f32 = 4.;
const THINK_GAP: f32 = 5.;
const PREPARE_W: f32 = 104.;
const PREPARE_DOT: f32 = 6.;
const PREPARE_MORPH_MS: u64 = 680;
/// The Skip chip nested in the polishing pill: a capsule concentric with
/// the pill's trailing end, so it reads as a control and not as a label.
const SKIP_H: f32 = 22.;
const SKIP_PAD: f32 = 10.;
const SKIP_INSET: f32 = 4.;
/// Leading padding of the polishing pill, which balances the chip's inset.
const SKIP_LEAD: f32 = 12.;
const THINK_DOTS_W: f32 = THINK_DOTS as f32 * THINK_DOT + (THINK_DOTS - 1) as f32 * THINK_GAP;

/// Opacities of the outgoing and incoming layer of a swap at progress `t`.
/// The first leaves before the second arrives, so two labels or two
/// silhouettes never sit on top of each other.
pub(super) fn swap(t: f32) -> (f32, f32) {
    (
        1. - (t / 0.5).clamp(0., 1.),
        ((t - 0.35) / 0.65).clamp(0., 1.),
    )
}

/// Where the processing pill's parts sit, measured from its outer edge.
#[derive(Clone, Copy, Debug, PartialEq)]
struct ThinkLayout {
    width: f32,
    /// Left edge of the dots; the label follows them.
    group_x: f32,
    /// Left edge of the Skip chip.
    skip_x: f32,
}

/// Lays out the dots, the label and the Skip chip. `polish` moves the label
/// from "Transcribing" to "Polishing" and `skip` brings the chip in; both
/// are eased by the caller, so every part moves on the same curve as the
/// capsule's width. Without the chip the group is centered in a pill no
/// narrower than the timed recording pill.
fn think_layout(
    timed_w: f32,
    transcribing_w: f32,
    polishing_w: f32,
    skip_w: f32,
    polish: f32,
    skip: f32,
) -> ThinkLayout {
    let label_w = transcribing_w + (polishing_w - transcribing_w) * polish;
    let lead = PILL_PAD + (SKIP_LEAD - PILL_PAD) * skip;
    let trail = PILL_PAD + (SKIP_INSET - PILL_PAD) * skip;
    let content = THINK_DOTS_W + THINK_LABEL_GAP + label_w + skip * (THINK_LABEL_GAP + skip_w);
    let width = (lead + content + trail).max(timed_w);
    let slack = width - (lead + content + trail);
    ThinkLayout {
        width,
        group_x: lead + slack / 2.,
        // The chip rides the trailing edge and settles the last few points.
        skip_x: width - trail - skip_w + (1. - skip) * 6.,
    }
}

/// Maps an RMS level to 0..1 linearly in decibels between
/// [`LEVEL_FLOOR_DB`] and [`LEVEL_CEIL_DB`].
fn bar_level(rms: f32) -> f32 {
    if rms <= 0. {
        return 0.;
    }
    let db = 20. * rms.log10();
    ((db - LEVEL_FLOOR_DB) / (LEVEL_CEIL_DB - LEVEL_FLOOR_DB)).clamp(0., 1.)
}

/// Height of bar `index` for an input level. Silence keeps a slow arch
/// that never flattens, so a quiet but live microphone still looks live;
/// under reduced motion the arch holds still.
fn live_target(rms: f32, index: usize, now_ms: f32, reduce: bool) -> f32 {
    let phase = now_ms * 0.012 * (1. + index as f32 * 0.37) + index as f32 * 1.7;
    let drift = 0.5 + 0.5 * phase.sin();
    let level = bar_level(rms);
    let speech = level * ENVELOPE[index] * (0.55 + 0.45 * drift);
    let swell = if reduce {
        0.8
    } else {
        0.6 + 0.4 * (0.5 + 0.5 * ((now_ms / 2400.) * TAU).sin())
    };
    speech + (1. - level) * BREATH * ENVELOPE[index] * swell
}

pub(crate) fn microphone_level_meter(rms: f32, reduce: bool, palette: &Palette) -> Div {
    div()
        .h(px(24.))
        .flex()
        .items_center()
        .gap(px(BAR_GAP))
        .children((0..BARS).map(|i| {
            div()
                .w(px(BAR_W))
                .h(px(BAR_MIN + live_target(rms, i, 0., reduce) * 12.))
                .rounded_full()
                .bg(palette.voice)
        }))
}

/// A zero-velocity handoff at both ends keeps the dots and bars continuous.
fn prepare_ease(t: f32) -> f32 {
    let t = t.clamp(0., 1.);
    t * t * t * (t * (t * 6. - 15.) + 10.)
}

/// The original three dots become bars 2, 5 and 8. The remaining bars
/// appear between them only after the capsule has begun expanding.
fn prepare_mark(index: usize, now_ms: f32, ready: f32, reduce: bool) -> (f32, f32, f32) {
    let seed = matches!(index, 2 | 5 | 8);
    let seed_index = (index as f32 - 2.) / 3.;
    let opacity = if seed {
        if reduce {
            0.75
        } else {
            let phase = now_ms / 1800. * TAU - seed_index * 0.85;
            0.32 + 0.68 * ((1. + phase.cos()) / 2.).powi(2)
        }
    } else {
        0.
    };
    let alpha = if seed {
        ready
    } else {
        prepare_ease((ready - 0.35) / 0.65)
    };
    (
        (seed_index - 1.) * 13.,
        if seed { PREPARE_DOT } else { BAR_W },
        opacity + (1. - opacity) * alpha,
    )
}

/// Opacity of transcribing dot `index`: a highlight that walks left to right.
fn think_opacity(index: usize, now_ms: f32, reduce: bool) -> f32 {
    if reduce {
        return 0.8;
    }
    let phase = ((now_ms / 1100.) - index as f32 * 0.18) * TAU;
    0.3 + 0.7 * (0.5 + 0.5 * phase.sin()).powi(2)
}

/// The pill's one-shot motion at an instant, layered over the waveform.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct PillMotion {
    pub(super) scale: f32,
    pub(super) alpha: f32,
    pub(super) bars: f32,
    /// Scale and opacity of the delivered check, once it shows.
    pub(super) check: Option<(f32, f32)>,
    pub(super) running: bool,
}

/// Samples the enter pop (scale .8 → 1 with overshoot), the dip when
/// recording hands over to processing (1 → .94 → 1), and the delivered
/// sequence: the bars fade, a check pops in, the pill swells to 1.06, holds,
/// then shrinks to .9 as it fades. Each input is the time since that event.
pub(super) fn pill_motion(
    entered: Option<Duration>,
    dipped: Option<Duration>,
    resolved: Option<Duration>,
    reduce: bool,
) -> PillMotion {
    let ms = |d: Duration| d.as_secs_f32() * 1000.;
    let mut m = PillMotion {
        scale: 1.,
        alpha: 1.,
        bars: 1.,
        check: None,
        running: false,
    };
    if reduce {
        if let Some(r) = resolved {
            m.bars = 0.;
            m.check = Some((1., 1.));
            if ms(r) >= RESOLVE_EXIT_AT as f32 {
                m.alpha = 0.;
            }
            // A one-shot wake draws the frame where the check goes.
        }
        return m;
    }
    if let Some(e) = entered.filter(|e| ms(*e) < DUR_BASE as f32) {
        m.scale *= 0.8 + 0.2 * ease_overshoot(progress(e, DUR_BASE));
        m.running = true;
    }
    if let Some(d) = dipped.map(ms).filter(|d| *d < 160.) {
        m.scale *= if d < 80. {
            1. - 0.06 * ease_out(d / 80.)
        } else {
            0.94 + 0.06 * ease_overshoot((d - 80.) / 80.)
        };
        m.running = true;
    }
    if let Some(r) = resolved {
        let r_ms = ms(r);
        m.bars = 1. - ease_snap(progress(r, DUR_SNAP));
        if r_ms > 60. {
            let k = ((r_ms - 60.) / 260.).clamp(0., 1.);
            m.check = Some((0.6 + 0.4 * ease_overshoot(k), ease_out(k)));
        }
        m.scale *= if r_ms < 80. {
            1. + 0.06 * ease_out(r_ms / 80.)
        } else if r_ms < 200. {
            1.06 - 0.06 * ease_overshoot((r_ms - 80.) / 120.)
        } else {
            1.
        };
        if r_ms >= RESOLVE_EXIT_AT as f32 {
            let x = ease_snap(((r_ms - RESOLVE_EXIT_AT as f32) / DUR_SNAP as f32).min(1.));
            m.scale *= 1. - 0.1 * x;
            m.alpha = 1. - x;
        }
        // The check is fully drawn at 320 ms and holds still until its exit.
        m.running = r_ms < 320. || (r_ms >= RESOLVE_EXIT_AT as f32 && r < RESOLVE);
    }
    m
}

/// Recording samples live levels and elapsed time even with reduced motion.
/// Preparation and processing only change continuously when their dots move.
fn waveform_needs_animation_frame(snapshot: &DictationSnapshot, reduce: bool) -> bool {
    snapshot.is_recording || (!reduce && snapshot.is_active())
}

impl PillView {
    fn update_lanes(&mut self, active: bool, level: Option<f32>, reduce: bool) {
        let now = Instant::now();
        let now_ms = now.duration_since(self.started).as_secs_f32() * 1000.;
        let frames = self
            .last_frame
            .map(|last| (now.duration_since(last).as_secs_f32() * 60.).min(4.))
            .unwrap_or(1.);
        self.last_frame = Some(now);
        if !active {
            return;
        }
        for i in 0..BARS {
            let target = level.map_or(0., |rms| live_target(rms, i, now_ms, reduce));
            let rate = if target > self.lanes[i] { RISE } else { FALL };
            self.lanes[i] += (target - self.lanes[i]) * (1. - (1. - rate).powf(frames));
        }
    }

    /// Keeps the recorded time current while the microphone is live.
    pub(super) fn update_recorded(&mut self, snapshot: &DictationSnapshot) {
        let now = Instant::now();
        if snapshot.is_recording
            && let Some(since) = self.recording_since
        {
            self.recorded = now.saturating_duration_since(since);
        }
    }

    /// The pill. Each phase has its own silhouette, so a still frame tells
    /// them apart without relying on color: a record dot beside level bars
    /// while the microphone is live, three breathing dots while it opens,
    /// and three labeled pulsing dots while transcribing.
    pub(super) fn render_pill(
        &mut self,
        snapshot: &DictationSnapshot,
        motion_fx: PillMotion,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = hud(cx);
        let reduce = cx.reduce_motion();
        let finishing = !snapshot.is_active();
        let recording = snapshot.is_recording;
        let polishing = snapshot.is_processing && snapshot.is_cleaning;
        // The finish keeps the silhouette it ended on: the meter must not
        // flash back under the delivered check, and a pill that has started
        // polishing never reads "Transcribing" again.
        if !finishing {
            self.think_latched = snapshot.is_processing;
            self.polish_latched &= snapshot.is_processing;
        }
        let (named, skippable) = polish_stage(
            self.polishing_since
                .filter(|_| polishing)
                .map(|since| since.elapsed()),
        );
        self.polish_latched |= named;
        let processing = self.think_latched;
        if waveform_needs_animation_frame(snapshot, reduce) {
            window.request_animation_frame();
        }
        let now_ms = self.started.elapsed().as_secs_f32() * 1000.;
        // A live microphone without a level yet is silence, not a flat row.
        let level = Dictation::global(cx)
            .read(cx)
            .level()
            .or(recording.then_some(0.));
        self.update_lanes(!finishing, level, reduce);
        let thinking = f32_tx(
            "pill-thinking",
            if processing { 1. } else { 0. },
            if reduce { DUR_FAST } else { DUR_BASE },
            ease_out,
            window,
            cx,
        );
        let (meter_alpha, think_alpha) = swap(thinking);
        let bars_alpha = meter_alpha * motion_fx.bars;
        let scale = motion_fx.scale;

        let timed_w = Self::timed_width(window);
        let ready = self.ready_at.map_or(0., |at| {
            if reduce {
                1.
            } else {
                prepare_ease(progress(at.elapsed(), PREPARE_MORPH_MS))
            }
        });
        // Processing may begin before the entrance finishes. It owns the
        // full timed width and must not revive the preparation dots.
        let ready = if processing { 1. } else { ready };
        let recording_w = PREPARE_W + (timed_w - PREPARE_W) * ready;
        let prepare_ms = self.ready_at.map_or(now_ms, |at| {
            at.duration_since(self.started).as_secs_f32() * 1000.
        });
        let bar_span = BARS as f32 * BAR_W + (BARS - 1) as f32 * BAR_GAP;
        let elapsed = format_elapsed(self.recorded);
        let timer_w = Self::label_width(&elapsed, window);
        // Reserve the longest timer in the capsule's outer width, but
        // center the visible group using the current timer's actual width.
        let meter_w = REC_DOT + REC_GAP + bar_span + TIMER_GAP + timer_w;
        let meter_left = (timed_w - meter_w) / 2.;
        // Absolute children start inside the 1px border. Their center
        // belongs to that inner box, including during the entrance pop.
        let content_w = recording_w * scale - 2.;
        let content_h = PILL_H * scale - 2.;
        let mut bars = div().absolute().inset_0();
        for i in 0..BARS {
            let (start_x, start_w, alpha) = prepare_mark(i, prepare_ms, ready, reduce);
            let end_x = meter_left + REC_DOT + REC_GAP + i as f32 * (BAR_W + BAR_GAP) + BAR_W / 2.
                - timed_w / 2.;
            let seed = matches!(i, 2 | 5 | 8);
            let x = if seed {
                start_x + (end_x - start_x) * ready
            } else {
                end_x
            };
            let w = start_w + (BAR_W - start_w) * ready;
            let start_h = if seed { PREPARE_DOT } else { BAR_W };
            let h = start_h + (BAR_MIN + self.lanes[i] * (BAR_MAX - BAR_MIN) - start_h) * ready;
            bars = bars.child(
                div()
                    .absolute()
                    .left(px(content_w / 2. + x - w / 2.))
                    .top(px((content_h - h) / 2.))
                    .w(px(w))
                    .h(px(h))
                    .rounded_full()
                    .bg(p.voice.opacity(alpha * bars_alpha)),
            );
        }
        let dot_color = p
            .destructive
            .opacity(bars_alpha * prepare_ease((ready - 0.22) / 0.68));
        let timer = div()
            .absolute()
            .left(px(content_w / 2. + timed_w / 2. - meter_left - timer_w))
            .top(px((content_h - LINE_SMALL) / 2.))
            .flex_none()
            .w(px(timer_w))
            .when(self.recording_since.is_none(), |d| d.invisible())
            .flex()
            .justify_end()
            .whitespace_nowrap()
            .text_size(px(TEXT_SMALL))
            .line_height(px(LINE_SMALL))
            .font_weight(FontWeight::MEDIUM)
            .font_features(tabular_nums())
            .text_color(
                p.foreground
                    .opacity(PILL_LABEL * bars_alpha * prepare_ease((ready - 0.42) / 0.58)),
            )
            .child(elapsed);
        let meter = div()
            .absolute()
            .inset_0()
            .child(
                div()
                    .absolute()
                    .left(px(content_w / 2. + meter_left - timed_w / 2.))
                    .top(px((content_h - REC_DOT) / 2.))
                    .size(px(REC_DOT))
                    .rounded_full()
                    .bg(dot_color),
            )
            .child(bars)
            .child(timer);

        // Transcribing names itself beside the dots, so the state reads
        // without hovering. It keeps the timed recording pill's width, so
        // the capsule does not jump sideways when recording ends.
        if !skippable {
            self.skip_hovered = false;
        }
        let labels = [
            t("dictation.pill.transcribing"),
            t("dictation.pill.polishing"),
        ];
        let label_w = [
            Self::label_width(&labels[0], window),
            Self::label_width(&labels[1], window),
        ];
        let skip_label = t("dictation.pill.skip");
        let skip_w = Self::label_width(&skip_label, window) + 2. * SKIP_PAD;
        let polish = f32_tx(
            "pill-polish",
            if self.polish_latched { 1. } else { 0. },
            DUR_BASE,
            ease_out,
            window,
            cx,
        );
        let skip_t = f32_tx(
            "skip-emerge",
            if skippable { 1. } else { 0. },
            if skippable { DUR_BASE } else { DUR_SNAP },
            if skippable { ease_out } else { ease_snap },
            window,
            cx,
        );
        let layout = think_layout(timed_w, label_w[0], label_w[1], skip_w, polish, skip_t);
        let think_alpha = think_alpha * motion_fx.bars;
        let think = (think_alpha > 0.001).then(|| {
            let mut dots = div().flex_none().flex().items_center().gap(px(THINK_GAP));
            for i in 0..THINK_DOTS {
                dots = dots.child(
                    div().size(px(THINK_DOT)).rounded_full().bg(p
                        .white
                        .opacity(think_opacity(i, now_ms, reduce) * think_alpha)),
                );
            }
            let (out, arrive) = swap(polish);
            let label = |text: &String, alpha: f32| {
                div()
                    .absolute()
                    .left_0()
                    .top_0()
                    .whitespace_nowrap()
                    .text_size(px(TEXT_SMALL))
                    .line_height(px(LINE_SMALL))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(p.foreground.opacity(PILL_LABEL * think_alpha * alpha))
                    .child(text.clone())
            };
            let group = div()
                .absolute()
                .left(px(layout.group_x - 1.))
                .top_0()
                .bottom_0()
                .flex()
                .items_center()
                .gap(px(THINK_LABEL_GAP))
                .child(dots)
                .child(
                    div()
                        .relative()
                        .flex_none()
                        .w(px(label_w[0].max(label_w[1])))
                        .h(px(LINE_SMALL))
                        .when(out > 0.001, |d| d.child(label(&labels[0], out)))
                        .when(arrive > 0.001, |d| d.child(label(&labels[1], arrive))),
                );
            // The chip stays for its exit but only acts while it is offered.
            let chip = (skip_t > 0.001).then(|| {
                let alpha = swap(skip_t).1 * think_alpha;
                let wash = color_tx(
                    "pill-skip-wash",
                    p.white.opacity(if self.skip_hovered { 0.2 } else { 0.11 }),
                    DUR_SNAP,
                    window,
                    cx,
                );
                div()
                    .id("pill-skip-cleanup")
                    .absolute()
                    .left(px(layout.skip_x - 1.))
                    .top(px((PILL_H - 2. - SKIP_H) / 2.))
                    .w(px(skip_w))
                    .h(px(SKIP_H))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .bg(wash.opacity(alpha))
                    .whitespace_nowrap()
                    .text_size(px(TEXT_SMALL))
                    .line_height(px(LINE_SMALL))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(p.foreground.opacity(0.92 * alpha))
                    .when(skippable, |d| {
                        d.keyboard_focusable(
                            "pill-skip-cleanup",
                            Role::Button,
                            t("dictation.pill.skipCleanup"),
                            cx,
                        )
                        .cursor_pointer()
                        .active(move |s| s.bg(p.white.opacity(0.26)))
                        .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                            this.skip_hovered = *hovered;
                            cx.notify();
                        }))
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.drag_origin = None;
                            cx.stop_propagation();
                            Dictation::global(cx).update(cx, |d, cx| d.skip_cleanup(cx));
                        }))
                    })
                    .child(skip_label.clone())
            });
            // A fixed-size stage centered in the pill, so the parts keep
            // their places while the capsule pops or dips around them.
            div()
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .relative()
                        .flex_none()
                        .w(px(layout.width - 2.))
                        .h(px(PILL_H - 2.))
                        .child(group)
                        .children(chip),
                )
        });
        let think_label = labels[usize::from(self.polish_latched)].clone();
        let think_w = layout.width;

        let check = motion_fx.check.map(|(scale, alpha)| {
            div()
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .child(
                    icon("check", 15., p.white.opacity(0.92 * alpha))
                        .with_transformation(Transformation::scale(size(scale, scale))),
                )
        });
        let width = recording_w + (think_w - recording_w) * thinking;
        // GPUI has no paint transforms on divs, so the pill scales by size
        // inside a fixed slot, which keeps it centered and the row still.
        let pill = div()
            .id("voice-pill")
            // VoiceOver hears what a press does, with the state as detail;
            // while transcribing the pill is only a status.
            .map(|d| {
                if processing {
                    d.role(Role::Status).aria_label(think_label)
                } else {
                    let detail = if recording {
                        crate::i18n::t_with(
                            "dictation.pill.elapsed",
                            &[("time", &format_elapsed(self.recorded))],
                        )
                    } else {
                        t("dictation.pill.starting")
                    };
                    d.role(Role::Button)
                        .aria_label(t("dictation.pill.finishDictation"))
                        .aria_description(detail)
                }
            })
            .relative()
            .flex_none()
            .w(px(width * scale))
            .h(px(PILL_H * scale))
            .flex()
            .items_center()
            .justify_center()
            .rounded_full()
            .border_1()
            .border_color(hud_edge())
            .bg(p.toast_surface)
            .shadow(hud_shadow(2., 6., 0., 0.28))
            .when(!processing && !finishing, |d| d.cursor_pointer())
            .child(meter)
            .children(think)
            .children(check)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, e: &MouseDownEvent, _, _| {
                    this.drag_origin = Some(e.position);
                }),
            )
            .on_mouse_move(cx.listener(|this, e: &MouseMoveEvent, window, _| {
                // Past a 5px threshold the press becomes a window drag.
                if let (Some(origin), Some(MouseButton::Left)) =
                    (this.drag_origin, e.pressed_button)
                {
                    let d = e.position - origin;
                    if f32::from(d.x).powi(2) + f32::from(d.y).powi(2) > 25. {
                        this.drag_origin = None;
                        window.start_window_move();
                    }
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    // A click on the finish or the delivered check starts nothing.
                    if this.drag_origin.take().is_some() && Self::snapshot(cx).is_active() {
                        Dictation::global(cx).update(cx, |d, cx| d.toggle_listening(cx));
                    }
                }),
            );
        div()
            .w(px(width))
            .h(px(PILL_H))
            .flex()
            .items_center()
            .justify_center()
            .child(pill)
            .into_any_element()
    }

    /// The width of a pill label set in the pill's label style.
    fn label_width(label: &str, window: &Window) -> f32 {
        let run = TextRun {
            len: label.len(),
            font: Font {
                weight: FontWeight::MEDIUM,
                features: tabular_nums(),
                ..font(FONT_UI)
            },
            color: Palette::dark().foreground,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let text = window.text_system().shape_line(
            SharedString::from(label.to_string()),
            px(TEXT_SMALL),
            &[run],
            None,
        );
        f32::from(text.width).ceil()
    }

    /// The timed recording pill's width, with a timer slot that already
    /// fits ten minutes and more.
    fn timed_width(window: &Window) -> f32 {
        PILL_W + TIMER_GAP + Self::label_width(TIMER_WIDEST, window)
    }

    /// The cancel control and how far it has emerged: it pops in from .8
    /// over 200 ms and leaves in 60 ms. At rest its glyph sits just below
    /// full strength; pointing at it brings it to full.
    pub(super) fn render_cancel(
        &mut self,
        visible: bool,
        recording: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> (AnyElement, f32) {
        let p = hud(cx);
        let reduce = cx.reduce_motion();
        let t_in = f32_tx(
            "cancel-emerge",
            if visible { 1. } else { 0. },
            if visible && !reduce {
                DUR_BASE
            } else {
                DUR_FAST
            },
            if visible { ease_out } else { ease_snap },
            window,
            cx,
        );
        let scale = if reduce { 1. } else { 0.8 + 0.2 * t_in };
        let glyph = if self.cancel_hovered {
            p.foreground
        } else {
            p.white.opacity(CANCEL_REST)
        };
        let button = div()
            .id("pill-cancel")
            .keyboard_focusable(
                "pill-cancel",
                Role::Button,
                t("dictation.pill.cancelDictation"),
                cx,
            )
            // Escape is bound only while the microphone is live.
            .when(recording, |d| d.aria_keyshortcuts("Escape"))
            .flex_none()
            .size(px(CANCEL_SIZE * scale))
            .flex()
            .items_center()
            .justify_center()
            .rounded_full()
            .border_1()
            .border_color(hud_edge())
            .bg(p.toast_surface)
            .shadow(hud_shadow(2., 6., 0., 0.28))
            .opacity(t_in)
            .cursor_pointer()
            .hover(move |s| s.bg(p.surface_raised))
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                this.cancel_hovered = *hovered;
                cx.notify();
            }))
            .child(
                icon("x", 12., glyph)
                    .with_transformation(Transformation::scale(size(scale, scale))),
            )
            .when(visible, |d| {
                d.on_click(cx.listener(move |_, _, _, cx| {
                    Dictation::global(cx).update(cx, |d, cx| {
                        if recording {
                            d.cancel_recording(cx)
                        } else {
                            d.cancel_processing(cx)
                        }
                    });
                }))
            });
        let slot = div()
            .size(px(CANCEL_SIZE))
            .flex()
            .items_center()
            .justify_center()
            .child(button)
            .into_any_element();
        (slot, t_in)
    }

    /// What the pointed-at control does, shown above the pill on hover. The
    /// processing pill already names itself, so it has no hint of its own.
    pub(super) fn render_hint(
        snapshot: &DictationSnapshot,
        on_cancel: bool,
        on_skip: bool,
        cx: &App,
    ) -> Option<Div> {
        let p = hud(cx);
        let caption = |text: String, color: Hsla| div().text_color(color).child(text);
        let row = div()
            .flex()
            .items_center()
            .gap(px(6.))
            .h(px(28.))
            .px(px(10.))
            .rounded(px(RADIUS_MD))
            .border_1()
            .border_color(hud_edge())
            .bg(p.toast_surface)
            .shadow(hud_shadow(2., 6., 0., 0.28))
            .text_size(px(TEXT_SMALL))
            .whitespace_nowrap();
        // Escape is bound globally for as long as the recording runs.
        let esc = snapshot.is_recording.then(|| kbd("Esc", &p));
        Some(
            if on_cancel && (snapshot.is_recording || snapshot.is_processing) {
                row.child(caption(t("dictation.pill.cancelDictation"), p.foreground))
                    .children(esc)
            } else if on_skip && snapshot.is_cleaning {
                row.child(caption(t("dictation.pill.skipHint"), p.foreground))
            } else if snapshot.is_recording {
                row.child(caption(t("dictation.pill.finish"), p.foreground))
                    .child(caption("·".into(), p.subtle_foreground))
                    .child(caption(t("dictation.pill.cancel"), p.muted_foreground))
                    .children(esc)
            } else if snapshot.is_processing {
                return None;
            } else {
                row.child(caption(t("dictation.pill.starting"), p.foreground))
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{
        BAR_MAX, BAR_MIN, BAR_W, BREATH, DUR_BASE, DictationSnapshot, ENVELOPE, RESOLVE,
        RESOLVE_EXIT_AT, bar_level, live_target, pill_motion, prepare_ease, prepare_mark,
        think_opacity, waveform_needs_animation_frame,
    };

    fn at(ms: u64) -> Option<Duration> {
        Some(Duration::from_millis(ms))
    }

    fn db(level: f32) -> f32 {
        10f32.powf(level / 20.)
    }

    #[test]
    fn fast_cleanup_never_shows_its_label_or_skip() {
        use super::{POLISH_LABEL_AFTER, SKIP_AFTER, polish_stage};
        assert_eq!(polish_stage(None), (false, false));
        // Measured against a healthy server: 170 ms to 1 s of cleanup.
        assert_eq!(polish_stage(at(170)), (false, false));
        assert_eq!(polish_stage(at(350)), (false, false));
        assert_eq!(polish_stage(at(1000)), (true, false));
        assert_eq!(polish_stage(Some(POLISH_LABEL_AFTER)), (true, false));
        assert_eq!(polish_stage(Some(SKIP_AFTER)), (true, true));
    }

    #[test]
    fn swapped_layers_never_overlap_at_full_strength() {
        assert_eq!(super::swap(0.), (1., 0.));
        assert_eq!(super::swap(1.), (0., 1.));
        for step in 0..=100 {
            let (out, arrive) = super::swap(step as f32 / 100.);
            assert!(out + arrive <= 1., "{out} + {arrive} at {step}");
        }
    }

    #[test]
    fn processing_layout_centers_the_label_and_docks_the_skip_chip() {
        use super::{PILL_PAD, SKIP_INSET, SKIP_LEAD, THINK_DOTS_W, THINK_LABEL_GAP, think_layout};
        let (timed, transcribing, polishing, skip) = (134., 70., 55., 43.);
        // Transcribing: the timed pill's width, with the group centered.
        let rest = think_layout(timed, transcribing, polishing, skip, 0., 0.);
        let group = THINK_DOTS_W + THINK_LABEL_GAP + transcribing;
        assert_eq!(rest.width, timed);
        assert!((rest.group_x - (timed - group) / 2.).abs() < 1e-4);
        // Polishing: the chip sits at its inset from the trailing edge.
        let full = think_layout(timed, transcribing, polishing, skip, 1., 1.);
        assert!((full.skip_x + skip + SKIP_INSET - full.width).abs() < 1e-4);
        assert!((full.group_x - SKIP_LEAD).abs() < 1e-4);
        let label_end = full.group_x + THINK_DOTS_W + THINK_LABEL_GAP + polishing;
        assert!(full.skip_x - label_end >= THINK_LABEL_GAP - 1e-4);
        // A long label widens the pill instead of crowding its padding.
        let long = think_layout(timed, 160., polishing, skip, 0., 0.);
        assert!(
            (long.width - (2. * PILL_PAD + THINK_DOTS_W + THINK_LABEL_GAP + 160.)).abs() < 1e-4
        );
        // The width changes monotonically while the chip arrives and leaves.
        let mut previous = think_layout(timed, transcribing, polishing, skip, 1., 0.).width;
        for step in 1..=20 {
            let width =
                think_layout(timed, transcribing, polishing, skip, 1., step as f32 / 20.).width;
            assert!(width >= previous - 1e-4);
            previous = width;
        }
    }

    #[test]
    fn waveform_frames_follow_live_levels_and_motion_preferences() {
        for snapshot in [
            DictationSnapshot::default(),
            DictationSnapshot {
                is_preparing: true,
                ..Default::default()
            },
            DictationSnapshot {
                is_processing: true,
                ..Default::default()
            },
            DictationSnapshot {
                is_recording: true,
                ..Default::default()
            },
        ] {
            assert_eq!(
                waveform_needs_animation_frame(&snapshot, false),
                snapshot.is_active()
            );
            assert_eq!(
                waveform_needs_animation_frame(&snapshot, true),
                snapshot.is_recording
            );
        }
    }

    #[test]
    fn delivered_check_requests_frames_only_while_changing() {
        for ms in [0, 100, 319, RESOLVE_EXIT_AT, RESOLVE_EXIT_AT + 60] {
            assert!(pill_motion(None, None, at(ms), false).running);
        }
        for ms in [320, 400, RESOLVE_EXIT_AT - 1, RESOLVE.as_millis() as u64] {
            assert!(!pill_motion(None, None, at(ms), false).running);
        }
        for ms in [0, 100, 400, RESOLVE_EXIT_AT] {
            assert!(!pill_motion(None, None, at(ms), true).running);
        }
    }

    #[test]
    fn bar_level_maps_decibels_onto_the_bars() {
        assert_eq!(bar_level(0.), 0.);
        assert_eq!(bar_level(db(-70.)), 0.);
        assert!((bar_level(db(-55.))).abs() < 1e-4);
        assert!((bar_level(db(-37.5)) - 0.5).abs() < 1e-4);
        assert!((bar_level(db(-20.)) - 1.).abs() < 1e-4);
        assert_eq!(bar_level(1.), 1.);
        // Close speech into a laptop mic (about -30 dBFS) clears 70%.
        assert!(bar_level(db(-30.)) > 0.7);
    }

    #[test]
    fn silence_only_breathes_and_speech_stands_out() {
        let peak = |rms: f32| {
            (0..240)
                .map(|t| live_target(rms, 4, t as f32 * 10., false))
                .fold(0., f32::max)
        };
        assert!(peak(0.) <= BREATH + 1e-4, "{}", peak(0.));
        assert!(peak(db(-30.)) > 0.6, "{}", peak(db(-30.)));
    }

    #[test]
    fn silence_is_an_arch_that_never_flattens() {
        for t in (0..2400).step_by(100) {
            let now = t as f32;
            let middle = live_target(0., 4, now, false);
            let edge = live_target(0., 0, now, false);
            assert!(middle > edge, "{middle} vs {edge} at {t}");
            assert!(middle >= BREATH * 0.6 - 1e-4, "{middle} at {t}");
        }
        let still = |now: f32| live_target(0., 4, now, true);
        assert_eq!(still(0.), still(1200.));
        assert!((still(0.) - BREATH * ENVELOPE[4] * 0.8).abs() < 1e-5);
    }

    #[test]
    fn transcribing_dots_walk_unless_motion_is_reduced() {
        let spread = |reduce: bool| {
            (0..3)
                .map(|i| think_opacity(i, 300., reduce))
                .fold((f32::MAX, 0f32), |(lo, hi), v| (lo.min(v), hi.max(v)))
        };
        let (lo, hi) = spread(false);
        assert!(hi - lo > 0.1, "{lo}..{hi}");
        let (lo, hi) = spread(true);
        assert_eq!(lo, hi);
    }

    #[test]
    fn preparation_shows_only_three_dots_until_the_microphone_is_ready() {
        for reduce in [false, true] {
            for now in [0., 900., 2300.] {
                let visible: Vec<_> = (0..10)
                    .filter(|&i| prepare_mark(i, now, 0., reduce).2 > 0.)
                    .collect();
                assert_eq!(visible, [2, 5, 8]);
                for i in 0..10 {
                    assert_eq!(prepare_mark(i, now, 1., reduce).2, 1.);
                }
            }
        }
        assert_eq!(
            prepare_mark(2, 0., 0., true),
            prepare_mark(2, 900., 0., true)
        );
    }

    #[test]
    fn preparation_handoff_has_stationary_endpoints() {
        assert_eq!(prepare_ease(-1.), 0.);
        assert_eq!(prepare_ease(2.), 1.);
        assert!(prepare_ease(0.001) < 0.00001);
        assert!(1. - prepare_ease(0.999) < 0.00001);
        let mut previous = 0.;
        for frame in 0..=100 {
            let value = prepare_ease(frame as f32 / 100.);
            assert!(value >= previous && (0. ..=1.).contains(&value));
            previous = value;
        }
    }

    #[test]
    fn silent_bars_stay_taller_than_wide() {
        // At their lowest the bars still read as a meter, not as dots.
        let height =
            |i: usize, now: f32| BAR_MIN + live_target(0., i, now, false) * (BAR_MAX - BAR_MIN);
        for t in (0..2400).step_by(100) {
            assert!(
                height(0, t as f32) >= BAR_W * 1.6,
                "{}",
                height(0, t as f32)
            );
            assert!(height(4, t as f32) >= 2. * BAR_W, "{}", height(4, t as f32));
        }
    }

    #[test]
    fn enter_pops_from_point_eight_past_one() {
        let start = pill_motion(at(0), None, None, false);
        assert!((start.scale - 0.8).abs() < 1e-3 && start.running);
        let peak = (1..20)
            .map(|i| pill_motion(at(i * 10), None, None, false).scale)
            .fold(0., f32::max);
        assert!(peak > 1.0, "{peak}");
        assert_eq!(pill_motion(at(DUR_BASE), None, None, false).scale, 1.);
    }

    #[test]
    fn processing_dips_then_recovers() {
        let low = pill_motion(None, at(80), None, false).scale;
        assert!((low - 0.94).abs() < 1e-3, "{low}");
        let done = pill_motion(None, at(160), None, false);
        assert_eq!(done.scale, 1.);
        assert!(!done.running);
    }

    #[test]
    fn delivered_swaps_bars_for_a_check_then_exits() {
        let early = pill_motion(None, None, at(80), false);
        assert!(early.scale > 1.05 && early.check.is_some() && early.bars < 0.5);
        let hold = pill_motion(None, None, at(400), false);
        assert_eq!((hold.scale, hold.alpha, hold.bars), (1., 1., 0.));
        assert_eq!(hold.check.map(|c| c.1), Some(1.));
        let gone = pill_motion(None, None, Some(RESOLVE), false);
        assert!(gone.alpha < 1e-3 && (gone.scale - 0.9).abs() < 1e-3 && !gone.running);
    }

    #[test]
    fn reduced_motion_keeps_scale_and_shows_the_check_still() {
        let m = pill_motion(at(10), at(10), at(100), true);
        assert_eq!((m.scale, m.alpha, m.bars), (1., 1., 0.));
        assert_eq!(m.check, Some((1., 1.)));
        assert_eq!(pill_motion(None, None, Some(RESOLVE), true).alpha, 0.);
    }
}
