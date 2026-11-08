//! The sidebar-specific morph used by T3 Code: Lucide PanelLeftClose ↔ PanelLeft.
//! Adapted from morphicons 1.7.1's polar interpolation and snappy spring.
//! See docs/third-party-notices.md for attribution and licenses.

use std::time::Instant;

use gpui_kit::*;

use super::components::icon;

const COUNT: usize = 64;
type Points = [[f64; 2]; COUNT];

fn shape(collapsed: bool) -> Points {
    std::array::from_fn(|i| {
        if collapsed {
            [9., 3. + 18. * i as f64 / 63.]
        } else if i <= 32 {
            let t = i as f64 / 32.;
            [16. - 3. * t, 15. - 3. * t]
        } else {
            let t = (i - 32) as f64 / 31.;
            [13. + 3. * t, 12. - 3. * t]
        }
    })
}

fn centroid(points: &Points) -> [f64; 2] {
    let sum = points
        .iter()
        .fold([0., 0.], |a, b| [a[0] + b[0], a[1] + b[1]]);
    [sum[0] / COUNT as f64, sum[1] / COUNT as f64]
}

struct Plan {
    from: Points,
    target: Points,
    ca: [f64; 2],
    cb: [f64; 2],
    angle: f64,
    log_scale: f64,
    score: f64,
}

impl Plan {
    fn aligned(from: Points, target: Points) -> Self {
        let ca = centroid(&from);
        let cb = centroid(&target);
        let (mut dot, mut cross, mut na, mut nb) = (0., 0., 0., 0.);
        for (a, b) in from.iter().zip(target.iter()) {
            let (ax, ay, bx, by) = (a[0] - ca[0], a[1] - ca[1], b[0] - cb[0], b[1] - cb[1]);
            dot += ax * bx + ay * by;
            cross += ax * by - ay * bx;
            na += ax * ax + ay * ay;
            nb += bx * bx + by * by;
        }
        let angle = cross.atan2(dot);
        let energy = angle.cos() * dot + angle.sin() * cross;
        let scale = if na > 1e-12 { energy / na } else { 1. }.max(1e-6);
        let residual =
            ((scale * scale * na - 2. * scale * energy + nb).max(0.) / nb.max(1e-12)).sqrt();
        Self {
            from,
            target,
            ca,
            cb,
            angle,
            log_scale: scale.ln(),
            score: residual + 0.05 * angle.abs() / std::f64::consts::PI,
        }
    }

    fn new(from: Points, target: Points) -> Self {
        let forward = Self::aligned(from, target);
        let reverse = Self::aligned(from, std::array::from_fn(|i| target[COUNT - 1 - i]));
        if reverse.score < forward.score {
            reverse
        } else {
            forward
        }
    }

    fn sample(&self, t: f64) -> Points {
        let scale = (self.log_scale * t).exp();
        let (sin, cos) = (self.angle * t).sin_cos();
        let (inv_sin, inv_cos) = (-self.angle).sin_cos();
        let inverse_scale = (-self.log_scale).exp();
        std::array::from_fn(|i| {
            let a = [self.from[i][0] - self.ca[0], self.from[i][1] - self.ca[1]];
            let b = [
                self.target[i][0] - self.cb[0],
                self.target[i][1] - self.cb[1],
            ];
            let bx = (b[0] * inv_cos - b[1] * inv_sin) * inverse_scale;
            let by = (b[0] * inv_sin + b[1] * inv_cos) * inverse_scale;
            let x = a[0] + (bx - a[0]) * t;
            let y = a[1] + (by - a[1]) * t;
            [
                self.ca[0] + (self.cb[0] - self.ca[0]) * t + scale * (x * cos - y * sin),
                self.ca[1] + (self.cb[1] - self.ca[1]) * t + scale * (x * sin + y * cos),
            ]
        })
    }
}

pub(super) struct SidebarIcon {
    collapsed: bool,
    points: Points,
    plan: Option<Plan>,
    progress: f64,
    velocity: f64,
    last_frame: Instant,
}

impl SidebarIcon {
    pub fn new(collapsed: bool) -> Self {
        Self {
            collapsed,
            points: shape(collapsed),
            plan: None,
            progress: 1.,
            velocity: 0.,
            last_frame: Instant::now(),
        }
    }

    fn advance(&mut self, dt: f64) {
        let Some(plan) = &self.plan else { return };
        let dt = dt.clamp(0., 0.1);
        let steps = (dt * 240.).ceil().clamp(1., 16.) as usize;
        let step = dt / steps as f64;
        for _ in 0..steps {
            self.velocity += (420. * (1. - self.progress) - 30. * self.velocity) * step;
            self.progress += self.velocity * step;
        }
        self.points = plan.sample(self.progress);
        if (1. - self.progress).abs() < 0.001 && self.velocity.abs() < 0.02 {
            self.points = shape(self.collapsed);
            self.progress = 1.;
            self.velocity = 0.;
            self.plan = None;
        }
    }

    pub fn update(&mut self, collapsed: bool, reduce_motion: bool) -> bool {
        let now = Instant::now();
        self.advance(now.duration_since(self.last_frame).as_secs_f64());
        self.last_frame = now;
        if reduce_motion {
            *self = Self::new(collapsed);
        } else if self.collapsed != collapsed {
            self.plan = Some(Plan::new(self.points, shape(collapsed)));
            self.collapsed = collapsed;
            self.progress = 0.;
            self.velocity = self.velocity.clamp(-14., 14.);
        }
        self.plan.is_some()
    }

    pub fn render(&self, color: Hsla) -> AnyElement {
        if self.plan.is_none() {
            return icon(
                if self.collapsed {
                    "panel-left-open"
                } else {
                    "panel-left-close"
                },
                16.,
                color,
            )
            .into_any_element();
        }
        let points = self.points;
        div()
            .relative()
            .size(px(16.))
            .flex_none()
            .child(icon("panel-left-open", 16., color))
            .child(
                canvas(
                    |_, _, _| {},
                    move |bounds, _, window, _| {
                        let scale = f32::from(bounds.size.width) / 24.;
                        let at = |p: [f64; 2]| {
                            point(
                                bounds.left() + px(p[0] as f32 * scale),
                                bounds.top() + px(p[1] as f32 * scale),
                            )
                        };
                        let mut path = PathBuilder::stroke(px(2. * scale));
                        path.move_to(at(points[0]));
                        for p in &points[1..] {
                            path.line_to(at(*p));
                        }
                        if let Ok(path) = path.build() {
                            window.paint_path(path, color);
                        }
                        // Round the two open ends, matching Lucide's round stroke caps.
                        for p in [points[0], points[COUNT - 1]] {
                            let center = at(p);
                            let r = px(scale);
                            let mut cap = PathBuilder::fill();
                            cap.move_to(point(center.x + r, center.y));
                            cap.arc_to(
                                point(r, r),
                                px(0.),
                                false,
                                true,
                                point(center.x - r, center.y),
                            );
                            cap.arc_to(
                                point(r, r),
                                px(0.),
                                false,
                                true,
                                point(center.x + r, center.y),
                            );
                            cap.close();
                            if let Ok(cap) = cap.build() {
                                window.paint_path(cap, color);
                            }
                        }
                    },
                )
                .absolute()
                .inset_0(),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::{Instant, Plan, SidebarIcon, shape};

    #[test]
    fn midpoint_matches_upstream_morphicons_1_7_1() {
        // Golden sample from morphicons' buildPlan + interpPolar for Lucide's pair.
        let plan = Plan::new(shape(false), shape(true));
        let middle = plan.sample(0.5);
        assert!((middle[0][0] - 12.907665326603885).abs() < 1e-9);
        assert!((middle[0][1] - 17.226413623681).abs() < 1e-9);
        assert!((plan.angle - 0.007572880836204991).abs() < 1e-12);
        assert!((plan.log_scale - 0.8753725112996396).abs() < 1e-12);
    }

    #[test]
    fn both_directions_settle_to_canonical_shapes() {
        let mut morph = SidebarIcon::new(false);
        for collapsed in [true, false] {
            assert!(morph.update(collapsed, false));
            for _ in 0..120 {
                morph.advance(1. / 120.);
            }
            assert!(morph.plan.is_none());
            assert_eq!(morph.points, shape(collapsed));
        }
    }

    #[test]
    fn interruption_preserves_rendered_shape_and_velocity() {
        let mut morph = SidebarIcon::new(false);
        morph.update(true, false);
        morph.advance(0.08);
        let before = morph.points;
        let velocity = morph.velocity;
        morph.last_frame = Instant::now();
        assert!(morph.update(false, false));
        for (a, b) in before.iter().zip(morph.points.iter()) {
            assert!((a[0] - b[0]).abs() < 0.01);
            assert!((a[1] - b[1]).abs() < 0.01);
        }
        assert!((morph.velocity - velocity).abs() < 0.01);
        for _ in 0..120 {
            morph.advance(1. / 120.);
        }
        assert_eq!(morph.points, shape(false));
    }

    #[test]
    fn reduce_motion_snaps_an_in_flight_morph() {
        let mut morph = SidebarIcon::new(false);
        morph.update(true, false);
        morph.advance(0.08);
        assert!(!morph.update(true, true));
        assert_eq!(morph.points, shape(true));
        assert_eq!(morph.velocity, 0.);
        assert!(!morph.update(false, true));
        assert_eq!(morph.points, shape(false));
    }
}
