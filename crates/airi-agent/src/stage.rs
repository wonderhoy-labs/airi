//! A stage is what one Android window shows: a private screen the size of that window, holding one
//! main Linux window that fills it, plus any dialogs and menus that belong to it.

use std::time::{Duration, Instant};

use smithay::{
    backend::{
        allocator::Fourcc,
        renderer::{
            Bind, Offscreen,
            damage::OutputDamageTracker,
            element::surface::WaylandSurfaceRenderElement,
            pixman::{PixmanError, PixmanRenderer},
        },
    },
    desktop::{Space, Window, WindowSurfaceType, space::render_output},
    output::{Mode, Output, PhysicalProperties, Scale, Subpixel},
    reexports::{
        pixman,
        wayland_server::{DisplayHandle, backend::GlobalId, protocol::wl_surface::WlSurface},
    },
    utils::{Logical, Physical, Point, Rectangle, Size, Transform},
    wayland::{fractional_scale::with_fractional_scale, seat::WaylandFocus},
};
use tracing::warn;

use crate::{
    net::{ConnId, Frame, FrameRect, Outgoing},
    state::State,
};

/// How many frames can be on their way before waiting for the phone to catch up. One would stall the
/// app for a round trip after every frame, and many would let the picture fall behind your finger.
const MAX_FRAMES_IN_FLIGHT: u32 = 2;

/// Past this many rectangles, sending one that covers them all is cheaper than sending each.
const MAX_RECTS: usize = 16;

/// How long a window that closed gets to be replaced before its Android window closes too. Plenty of
/// apps swap a splash screen for their real window.
pub const REPLACEMENT_GRACE: Duration = Duration::from_millis(1500);

/// How long an app gets to open a window after being launched.
pub const LAUNCH_TIMEOUT: Duration = Duration::from_secs(90);

pub struct Waiting {
    pub until: Instant,
    pub launched_pid: Option<u32>,
    pub since_launch: bool,
}

pub struct Stage {
    pub conn: ConnId,
    pub out: Outgoing,
    pub output: Output,
    pub global: GlobalId,
    pub space: Space<Window>,
    pub damage: OutputDamageTracker,
    pub image: pixman::Image<'static, 'static>,
    /// The size of the Android window, in pixels.
    pub view: Size<i32, Physical>,
    /// The size being drawn. It's only bigger than the view if an app won't fit, and then Android
    /// scales the picture down.
    pub buffer: Size<i32, Physical>,
    pub scale: f64,
    pub desktop_id: String,
    pub main: Option<u32>,
    pub children: Vec<u32>,
    pub overrides: Vec<Window>,
    pub waiting: Option<Waiting>,
    pub visible: bool,
    pub focused: bool,
    pub dirty: bool,
    pub redraw_all: bool,
    pub frames_in_flight: u32,
    pub seq: u32,
}

fn make_output(name: String, size: Size<i32, Physical>, scale: f64) -> Output {
    let output = Output::new(
        name,
        PhysicalProperties {
            size: (0, 0).into(),
            subpixel: Subpixel::Unknown,
            make: "Airi".into(),
            model: "Android window".into(),
        },
    );
    let mode = Mode { size, refresh: 60_000 };
    output.change_current_state(
        Some(mode),
        Some(Transform::Normal),
        Some(Scale::Fractional(scale)),
        Some((0, 0).into()),
    );
    output.set_preferred(mode);
    output
}

impl Stage {
    pub fn new(
        conn: ConnId,
        out: Outgoing,
        dh: &DisplayHandle,
        renderer: &mut PixmanRenderer,
        view: Size<i32, Physical>,
        scale: f64,
        desktop_id: String,
    ) -> Result<Self, PixmanError> {
        let output = make_output(format!("android-{conn}"), view, scale);
        let global = output.create_global::<State>(dh);
        let mut space = Space::default();
        space.map_output(&output, (0, 0));
        let image = renderer.create_buffer(Fourcc::Abgr8888, (view.w, view.h).into())?;
        Ok(Self {
            conn,
            out,
            damage: OutputDamageTracker::from_output(&output),
            output,
            global,
            space,
            image,
            view,
            buffer: view,
            scale,
            desktop_id,
            main: None,
            children: Vec::new(),
            overrides: Vec::new(),
            waiting: None,
            visible: true,
            focused: false,
            dirty: true,
            redraw_all: true,
            frames_in_flight: 0,
            seq: 0,
        })
    }

    pub fn logical_size(&self) -> Size<i32, Logical> {
        logical(self.buffer, self.scale)
    }

    /// The size a window filling the Android window should be.
    pub fn view_logical_size(&self) -> Size<i32, Logical> {
        logical(self.view, self.scale)
    }

    pub fn contains_surface(&self, root: &WlSurface) -> bool {
        self.window_for_surface(root).is_some()
    }

    pub fn window_for_surface(&self, root: &WlSurface) -> Option<Window> {
        self.space.elements().find(|window| window.wl_surface().as_deref() == Some(root)).cloned()
    }

    /// Changes the size or scale. The picture is redrawn from scratch at the new size.
    pub fn resize(
        &mut self,
        renderer: &mut PixmanRenderer,
        view: Size<i32, Physical>,
        scale: f64,
    ) -> Result<(), PixmanError> {
        self.view = view;
        self.scale = scale;
        self.set_buffer_size(renderer, view)?;
        self.output.change_current_state(None, None, Some(Scale::Fractional(scale)), None);
        self.damage = OutputDamageTracker::from_output(&self.output);
        self.redraw_all = true;
        self.dirty = true;
        Ok(())
    }

    /// Grows the drawing area so that a window that refuses to shrink is still all there.
    pub fn fit_to(&mut self, renderer: &mut PixmanRenderer, content: Size<i32, Logical>) {
        // A window that merely fills the view rounds up to a pixel or two more than it. Growing for
        // that would make Android rescale the picture and soften everything in it.
        let limit = self.view_logical_size();
        let fit = |content: i32, limit: i32, view: i32| {
            if content > limit { (content as f64 * self.scale).ceil() as i32 } else { view }
        };
        let needed = Size::<i32, Physical>::from((
            fit(content.w, limit.w, self.view.w),
            fit(content.h, limit.h, self.view.h),
        ));
        // Going back to the view size happens when the window shrinks again, so this handles both.
        if needed != self.buffer {
            if let Err(err) = self.set_buffer_size(renderer, needed) {
                warn!(?err, "couldn't resize a window's buffer");
            }
            self.damage = OutputDamageTracker::from_output(&self.output);
            self.redraw_all = true;
            self.dirty = true;
        }
    }

    fn set_buffer_size(
        &mut self,
        renderer: &mut PixmanRenderer,
        size: Size<i32, Physical>,
    ) -> Result<(), PixmanError> {
        if size == self.buffer && self.image.width() == size.w as usize && self.image.height() == size.h as usize {
            return Ok(());
        }
        self.image = renderer.create_buffer(Fourcc::Abgr8888, (size.w, size.h).into())?;
        self.buffer = size;
        let mode = Mode { size, refresh: 60_000 };
        self.output.change_current_state(Some(mode), None, None, None);
        self.output.set_preferred(mode);
        Ok(())
    }

    pub fn can_draw(&self) -> bool {
        self.visible && self.dirty && self.frames_in_flight < MAX_FRAMES_IN_FLIGHT
    }

    pub fn map(&mut self, window: Window, location: Point<i32, Logical>, activate: bool) {
        self.space.map_element(window, location, activate);
        self.dirty = true;
    }

    /// Draws what changed, sends it to Android, and tells apps they can draw their next frame.
    pub fn draw(&mut self, renderer: &mut PixmanRenderer, time: Duration) {
        self.dirty = false;
        self.space.refresh();

        let age = if self.redraw_all { 0 } else { 1 };
        let damage: Option<Vec<Rectangle<i32, Physical>>> = {
            let mut target = match renderer.bind(&mut self.image) {
                Ok(target) => target,
                Err(err) => {
                    warn!(?err, "couldn't bind a window's buffer");
                    return;
                }
            };
            let result = render_output::<_, WaylandSurfaceRenderElement<PixmanRenderer>, _, _>(
                &self.output,
                renderer,
                &mut target,
                1.0,
                age,
                [&self.space],
                &[],
                &mut self.damage,
                [0.0, 0.0, 0.0, 1.0],
            );
            match result {
                Ok(result) => {
                    let _ = result.sync.wait();
                    result.damage.cloned()
                }
                Err(err) => {
                    warn!(?err, "couldn't draw a window");
                    None
                }
            }
        };

        let output = &self.output;
        for window in self.space.elements() {
            window.send_frame(output, time, Some(Duration::ZERO), |_, _| Some(output.clone()));
        }

        let full = Rectangle::from_size(self.buffer);
        let rects = if self.redraw_all {
            vec![full]
        } else {
            match damage {
                Some(rects) if !rects.is_empty() => simplify_damage(rects, full),
                _ => return,
            }
        };
        self.redraw_all = false;
        self.send_rects(&rects);
    }

    /// Copies the changed pixels out, so the compressing can happen on another thread.
    fn send_rects(&mut self, rects: &[Rectangle<i32, Physical>]) {
        let stride = self.image.stride();
        let total = stride * self.image.height();
        // Safety: the image owns this much memory, and nothing else writes to it during this call.
        let pixels = unsafe { std::slice::from_raw_parts(self.image.data() as *const u8, total) };
        let frame_rects = rects
            .iter()
            .map(|rect| {
                let row = rect.size.w as usize * 4;
                let mut copied = Vec::with_capacity(row * rect.size.h as usize);
                for y in rect.loc.y as usize..(rect.loc.y + rect.size.h) as usize {
                    let start = y * stride + rect.loc.x as usize * 4;
                    copied.extend_from_slice(&pixels[start..start + row]);
                }
                FrameRect {
                    x: rect.loc.x as u32,
                    y: rect.loc.y as u32,
                    width: rect.size.w as u32,
                    height: rect.size.h as u32,
                    pixels: copied,
                }
            })
            .collect();
        self.seq = self.seq.wrapping_add(1);
        self.frames_in_flight += 1;
        self.out.send_frame(Frame {
            seq: self.seq,
            width: self.buffer.w as u32,
            height: self.buffer.h as u32,
            rects: frame_rects,
        });
    }

    /// Finds the surface under a point, in the stage's logical coordinates.
    pub fn surface_under(&self, point: Point<f64, Logical>) -> Option<(WlSurface, Point<f64, Logical>)> {
        let (window, location) = self.space.element_under(point)?;
        let (surface, offset) = window.surface_under(point - location.to_f64(), WindowSurfaceType::ALL)?;
        Some((surface, (offset + location).to_f64()))
    }

    pub fn update_surface_scale(&self) {
        for window in self.space.elements() {
            let scale = self.scale;
            window.with_surfaces(|_, states| {
                with_fractional_scale(states, |fractional| fractional.set_preferred_scale(scale));
            });
        }
    }
}

/// Rounds up, because a window a fraction of a pixel too small leaves a gap down one edge.
fn logical(size: Size<i32, Physical>, scale: f64) -> Size<i32, Logical> {
    ((size.w as f64 / scale).ceil() as i32, (size.h as f64 / scale).ceil() as i32).into()
}

fn simplify_damage(
    mut rects: Vec<Rectangle<i32, Physical>>,
    full: Rectangle<i32, Physical>,
) -> Vec<Rectangle<i32, Physical>> {
    rects.retain_mut(|rect| match rect.intersection(full) {
        Some(clipped) if !clipped.is_empty() => {
            *rect = clipped;
            true
        }
        _ => false,
    });
    if rects.len() > MAX_RECTS {
        let mut merged = rects[0];
        for rect in &rects[1..] {
            merged = merged.merge(*rect);
        }
        return vec![merged];
    }
    rects
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: i32, y: i32, w: i32, h: i32) -> Rectangle<i32, Physical> {
        Rectangle::new((x, y).into(), (w, h).into())
    }

    #[test]
    fn damage_is_clipped_to_the_frame() {
        let full = rect(0, 0, 100, 100);
        let out = simplify_damage(vec![rect(90, 90, 50, 50), rect(200, 200, 5, 5)], full);
        assert_eq!(out, vec![rect(90, 90, 10, 10)]);
    }

    #[test]
    fn lots_of_damage_becomes_one_rectangle() {
        let full = rect(0, 0, 1000, 1000);
        let many: Vec<_> = (0..MAX_RECTS as i32 + 1).map(|i| rect(i * 10, i * 10, 5, 5)).collect();
        let out = simplify_damage(many, full);
        assert_eq!(out, vec![rect(0, 0, MAX_RECTS as i32 * 10 + 5, MAX_RECTS as i32 * 10 + 5)]);
    }

    #[test]
    fn logical_sizes_round_up() {
        assert_eq!(logical((1080, 2410).into(), 2.0), (540, 1205).into());
        assert_eq!(logical((1080, 2200).into(), 2.625), (412, 839).into());
    }
}
