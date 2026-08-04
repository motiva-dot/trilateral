//! The debug view's window and event loop.
//!
//! # Frame-0 feedback lives here, not in the simulation
//! PRD pillar #4 and TECH_SPEC §5.1: the acknowledgement a player sees when
//! they right-click must appear on the *same frame as the click*, while the
//! command itself executes `input_delay` ticks later on every client. That
//! split is the whole trick — the marker is presentation state that never
//! touches `SimState`, so it costs nothing in determinism and buys the entire
//! perception of responsiveness.
//!
//! Implementing it now, in a throwaway view, is deliberate. Retrofitting
//! Frame-0 feedback later means auditing every input path for places that
//! assumed the simulation answered immediately.
//!
//! # The only wall clock in the project
//! `Instant` is banned in simulation crates (§1.2). It appears here because
//! the accumulator needs real elapsed time to decide how many ticks are owed.
//! What crosses into the simulation is a *tick count*, never a duration.

use std::sync::Arc;
use std::time::Instant;

use sim_core::{Command, IssuedCommand, Registries, SimState, Tick};
use sim_systems::{SimContext, tick};
use trilateral_fixed::{Fixed, FixedVec2};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowId};

use crate::render_state::Instance;
use crate::{Camera, RenderState, Renderer, StepAccumulator};

/// Ticks between issuing a command and executing it. TECH_SPEC §5.1 makes this
/// adaptive 2–4 for network play; offline it is a constant, so that local feel
/// matches networked feel rather than being deceptively snappier.
const INPUT_DELAY_TICKS: u64 = 3;

/// How long the Frame-0 click marker stays visible.
const MARKER_TICKS: u64 = 12;

/// Camera pan speed, tiles per second at zoom 1.
const PAN_TILES_PER_SEC: f64 = 24.0;

#[derive(Default)]
struct Held {
    left: bool,
    right: bool,
    up: bool,
    down: bool,
}

pub struct DebugApp {
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
    state: SimState,
    reg: Registries,
    ctx: SimContext,
    view: RenderState,
    camera: Camera,
    accum: StepAccumulator,
    last_frame: Instant,
    held: Held,
    cursor: [f64; 2],
    drag_from: Option<[f64; 2]>,
    /// Frame-0 click marker: where, and the tick it expires.
    marker: Option<([f64; 2], u64)>,
    paused: bool,
}

impl DebugApp {
    pub fn new(state: SimState, reg: Registries, world_tiles: i32) -> DebugApp {
        let capacity = state.c.capacity();
        let ctx = SimContext::new(world_tiles, &reg);
        let mut view = RenderState::new(capacity);
        view.capture(&state, &reg);
        let mut camera = Camera::new();
        camera.centre = [world_tiles as f64 / 2.0, world_tiles as f64 / 2.0];
        DebugApp {
            window: None,
            renderer: None,
            state,
            reg,
            ctx,
            view,
            camera,
            accum: StepAccumulator::new(),
            last_frame: Instant::now(),
            held: Held::default(),
            cursor: [0.0, 0.0],
            drag_from: None,
            marker: None,
            paused: false,
        }
    }

    fn advance(&mut self, real_seconds: f64) {
        if self.paused {
            return;
        }
        let owed = self.accum.advance(real_seconds);
        for _ in 0..owed {
            tick(&mut self.state, &self.reg, &mut self.ctx);
            // Capture once per tick, never per frame — the mirror is what the
            // interpolation lerps between.
            self.view.capture(&self.state, &self.reg);
        }
        if let Some((_, expires)) = self.marker
            && self.state.clock.tick.0 >= expires
        {
            self.marker = None;
        }
    }

    fn pan(&mut self, real_seconds: f64) {
        let d = PAN_TILES_PER_SEC * real_seconds / self.camera.zoom;
        let mut dx = 0.0;
        let mut dy = 0.0;
        if self.held.left {
            dx -= d;
        }
        if self.held.right {
            dx += d;
        }
        if self.held.up {
            dy -= d;
        }
        if self.held.down {
            dy += d;
        }
        if dx != 0.0 || dy != 0.0 {
            self.camera.pan(dx, dy);
        }
    }

    /// Right-click: acknowledge instantly, execute later.
    fn order_move(&mut self, screen: [f64; 2]) {
        let world = self.camera.screen_to_world(screen);
        // Frame 0: the marker exists before any command has been validated,
        // let alone executed. This is what the player actually perceives.
        self.marker = Some((world, self.state.clock.tick.0 + MARKER_TICKS));

        let at = Tick(self.state.clock.tick.0 + INPUT_DELAY_TICKS);
        let target = FixedVec2::new(Fixed::from_f64(world[0]), Fixed::from_f64(world[1]));
        let selected: Vec<u32> = self.view.selected.clone();
        for idx in selected {
            // One command per entity — see sim_core::command.
            let Some(h) = self.state.entities.iter_live().find(|h| h.index == idx) else {
                continue;
            };
            let owner = self.state.c.owner[idx as usize];
            let _ = self.state.ingest(IssuedCommand {
                tick: at,
                player: owner,
                subject: h,
                command: Command::Move { target },
            });
        }
    }

    fn draw(&mut self) {
        let Some(renderer) = self.renderer.as_mut() else {
            return;
        };
        let (w, h) = renderer.size();
        self.camera.viewport = [w as f32, h as f32];

        let alpha = self.accum.alpha();
        let mut frame: Vec<Instance> = self.view.instances(&self.camera, alpha).to_vec();

        // The click marker, drawn last so it sits on top. Presentation-only —
        // it is not in SimState and never will be.
        if let Some((at, _)) = self.marker {
            let scale = self.camera.tile_to_ndc_scale();
            frame.push(Instance {
                ndc: self.camera.world_to_ndc(at),
                half: [0.35 * scale[0], 0.35 * scale[1]],
                colour: [1.0, 1.0, 0.55, 0.85],
            });
        }
        renderer.render(&frame);
    }
}

impl ApplicationHandler for DebugApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attrs = Window::default_attributes()
            .with_title("TRILATERAL — debug view (Phase 3.5)")
            .with_inner_size(winit::dpi::LogicalSize::new(1280.0, 720.0));
        let window = Arc::new(
            event_loop
                .create_window(attrs)
                .expect("failed to create window"),
        );
        self.renderer = Some(Renderer::new(window.clone()));
        self.window = Some(window);
        self.last_frame = Instant::now();
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),

            WindowEvent::Resized(size) => {
                if let Some(r) = self.renderer.as_mut() {
                    r.resize(size.width, size.height);
                }
            }

            WindowEvent::CursorMoved { position, .. } => {
                self.cursor = [position.x, position.y];
            }

            WindowEvent::MouseWheel { delta, .. } => {
                let notches = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y as f64,
                    MouseScrollDelta::PixelDelta(p) => p.y / 120.0,
                };
                if notches != 0.0 {
                    self.camera.zoom_by(1.15f64.powf(notches));
                }
            }

            WindowEvent::MouseInput { state, button, .. } => match (button, state) {
                (MouseButton::Left, ElementState::Pressed) => {
                    self.drag_from = Some(self.cursor);
                }
                (MouseButton::Left, ElementState::Released) => {
                    let from = self.drag_from.take().unwrap_or(self.cursor);
                    let a = self.camera.screen_to_world(from);
                    let b = self.camera.screen_to_world(self.cursor);
                    let dragged = (a[0] - b[0]).abs() > 0.25 || (a[1] - b[1]).abs() > 0.25;
                    self.view.selected = if dragged {
                        self.view.pick_box(a, b)
                    } else {
                        self.view.pick(b, 0.3).into_iter().collect()
                    };
                }
                (MouseButton::Right, ElementState::Pressed) => {
                    self.order_move(self.cursor);
                }
                _ => {}
            },

            WindowEvent::KeyboardInput { event, .. } => {
                let down = event.state == ElementState::Pressed;
                if let PhysicalKey::Code(code) = event.physical_key {
                    match code {
                        KeyCode::KeyA | KeyCode::ArrowLeft => self.held.left = down,
                        KeyCode::KeyD | KeyCode::ArrowRight => self.held.right = down,
                        KeyCode::KeyW | KeyCode::ArrowUp => self.held.up = down,
                        KeyCode::KeyS | KeyCode::ArrowDown => self.held.down = down,
                        KeyCode::Escape if down => event_loop.exit(),
                        KeyCode::Space if down => self.paused = !self.paused,
                        _ => {}
                    }
                }
            }

            WindowEvent::RedrawRequested => {
                let now = Instant::now();
                let dt = now.duration_since(self.last_frame).as_secs_f64();
                self.last_frame = now;
                self.pan(dt);
                self.advance(dt);
                self.draw();
            }

            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        // Redraw continuously. A game loop is not event-driven — the
        // simulation must advance whether or not the mouse moved.
        if let Some(w) = self.window.as_ref() {
            w.request_redraw();
        }
    }
}
