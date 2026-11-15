//! `ymux placeholder`: one fully transparent, undecorated window that keeps a
//! tile's spot open (spec 5.7). App id `ymux-placeholder`; it exits when the
//! compositor closes it or the connection is lost.

use anyhow::{Context, Result};
use smithay_client_toolkit::reexports::client::{
    globals::registry_queue_init,
    protocol::{wl_output, wl_shm, wl_surface},
    Connection, QueueHandle,
};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    delegate_registry,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    shell::{
        xdg::{
            window::{Window, WindowConfigure, WindowDecorations, WindowHandler},
            XdgShell,
        },
        WaylandSurface,
    },
    shm::{slot::SlotPool, Shm, ShmHandler},
};

pub const APP_ID: &str = crate::model::PLACEHOLDER_CLASS;

/// Bytes of an ARGB8888 buffer; never zero, so a 0x0 configure still gets a buffer.
pub fn buffer_len(w: u32, h: u32) -> usize {
    w.max(1) as usize * h.max(1) as usize * 4
}

struct Placeholder {
    registry_state: RegistryState,
    output_state: OutputState,
    shm: Shm,
    pool: SlotPool,
    window: Window,
    exit: bool,
}

pub fn run() -> Result<()> {
    let conn = Connection::connect_to_env().context("cannot connect to the Wayland display")?;
    let (globals, mut queue) = registry_queue_init(&conn).context("wayland registry")?;
    let qh = queue.handle();
    let compositor = CompositorState::bind(&globals, &qh).context("wayland: wl_compositor missing")?;
    let xdg = XdgShell::bind(&globals, &qh).context("wayland: xdg_wm_base missing")?;
    let shm = Shm::bind(&globals, &qh).context("wayland: wl_shm missing")?;

    let surface = compositor.create_surface(&qh);
    let window = xdg.create_window(surface, WindowDecorations::None, &qh);
    window.set_title("ymux placeholder");
    window.set_app_id(APP_ID);
    window.commit();

    let pool = SlotPool::new(buffer_len(1, 1), &shm).context("shm pool")?;
    let mut state = Placeholder {
        registry_state: RegistryState::new(&globals),
        output_state: OutputState::new(&globals, &qh),
        shm,
        pool,
        window,
        exit: false,
    };
    while !state.exit {
        if queue.blocking_dispatch(&mut state).is_err() {
            break; // connection lost: nothing left to hold open
        }
    }
    Ok(())
}

impl Placeholder {
    /// Attaches a fully transparent buffer of the configured size and commits.
    fn draw(&mut self, w: u32, h: u32) {
        let (w, h) = (w.max(1), h.max(1));
        if let Ok((buffer, canvas)) = self.pool.create_buffer(w as i32, h as i32, w as i32 * 4, wl_shm::Format::Argb8888) {
            canvas.fill(0);
            let surface = self.window.wl_surface();
            surface.damage_buffer(0, 0, w as i32, h as i32);
            if buffer.attach_to(surface).is_ok() {
                self.window.commit();
            }
        }
    }
}

impl WindowHandler for Placeholder {
    fn request_close(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &Window) {
        self.exit = true;
    }

    fn configure(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &Window, configure: WindowConfigure, _: u32) {
        let w = configure.new_size.0.map(|v| v.get()).unwrap_or(1);
        let h = configure.new_size.1.map(|v| v.get()).unwrap_or(1);
        self.draw(w, h);
    }
}

impl CompositorHandler for Placeholder {
    fn scale_factor_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: i32) {}
    fn transform_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: wl_output::Transform) {}
    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {}
    fn surface_enter(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
    fn surface_leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
}

impl OutputHandler for Placeholder {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl ShmHandler for Placeholder {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl ProvidesRegistryState for Placeholder {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState];
}

delegate_registry!(Placeholder);
smithay_client_toolkit::delegate_dispatch2!(Placeholder);
