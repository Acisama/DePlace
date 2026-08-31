use std::time::{Duration, Instant};

use iced::widget::canvas::{Frame, Path};

use crate::common::*;

use super::SidebarMessage;

const PILL_ANIM: Duration = Duration::from_millis(100);

#[derive(Clone, Copy)]
struct PillAnim {
    from: f32,
    to: f32,
    start: Instant,
}

impl PillAnim {
    fn value(&self, now: Instant) -> f32 {
        let t = (now.duration_since(self.start).as_secs_f32() / PILL_ANIM.as_secs_f32())
            .clamp(0.0, 1.0);
        let eased = t * t * (3.0 - 2.0 * t);
        self.from + (self.to - self.from) * eased
    }
}

#[derive(Default)]
pub struct PillState {
    anim: Option<PillAnim>,
}

pub struct PillCanvas {
    pub target: f32,
    pub width: f32,
    pub color: Color,
    pub radius: f32,
}

impl canvas::Program<SidebarMessage> for PillCanvas {
    type State = PillState;

    fn update(
        &self,
        state: &mut PillState,
        event: &canvas::Event,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Option<canvas::Action<SidebarMessage>> {
        let canvas::Event::Window(window::Event::RedrawRequested(now)) = event else {
            return None;
        };

        match &mut state.anim {
            None => {
                // First frame ever - snap to the current target, don't animate in from 0.
                state.anim = Some(PillAnim {
                    from: self.target,
                    to: self.target,
                    start: *now,
                });
            }
            Some(anim) if anim.to != self.target => {
                // Target changed - restart the tween from wherever we currently are,
                // so an interrupted transition doesn't jump.
                let current = anim.value(*now);
                *anim = PillAnim {
                    from: current,
                    to: self.target,
                    start: *now,
                };
            }
            _ => {}
        }
        None
    }

    fn draw(
        &self,
        state: &PillState,
        renderer: &Renderer,
        _theme: &IcedTheme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let height = state
            .anim
            .map(|a| a.value(Instant::now()))
            .unwrap_or(self.target);
        let mut frame = Frame::new(renderer, bounds.size());
        let offset = (bounds.height - height) / 2.0;

        let path = Path::rounded_rectangle(
            Point::new(0.0, offset),
            Size::new(self.width, height),
            self.radius.into(),
        );
        frame.fill(&path, self.color);
        vec![frame.into_geometry()]
    }
}
