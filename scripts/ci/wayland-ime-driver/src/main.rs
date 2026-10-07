use std::{io::Write, os::fd::AsFd};
use wayland_client::{
    Connection, Dispatch, QueueHandle, delegate_noop,
    protocol::{
        wl_buffer, wl_compositor, wl_keyboard, wl_registry, wl_seat, wl_shm, wl_shm_pool,
        wl_surface,
    },
};

fn log(message: impl std::fmt::Display) {
    println!("{message}");
    let _ = std::io::stdout().flush();
}

use wayland_protocols_misc::zwp_input_method_v2::client::{
    zwp_input_method_keyboard_grab_v2, zwp_input_method_manager_v2, zwp_input_method_v2,
    zwp_input_popup_surface_v2,
};

const POPUP_WIDTH: u32 = 48;
const POPUP_HEIGHT: u32 = 22;

#[derive(Default)]
struct State {
    seat: Option<wl_seat::WlSeat>,
    compositor: Option<wl_compositor::WlCompositor>,
    shm: Option<wl_shm::WlShm>,
    surface: Option<wl_surface::WlSurface>,
    shm_file: Option<std::fs::File>,
    buffer: Option<wl_buffer::WlBuffer>,
    manager: Option<zwp_input_method_manager_v2::ZwpInputMethodManagerV2>,
    input_method: Option<zwp_input_method_v2::ZwpInputMethodV2>,
    popup: Option<zwp_input_popup_surface_v2::ZwpInputPopupSurfaceV2>,
    keyboard_grab: Option<zwp_input_method_keyboard_grab_v2::ZwpInputMethodKeyboardGrabV2>,
    serial: u32,
    active: bool,
    preedit_sent: bool,
    commit_sent: bool,
    committed: usize,
}

impl State {
    fn create_popup(
        &mut self,
        input_method: &zwp_input_method_v2::ZwpInputMethodV2,
        qh: &QueueHandle<Self>,
    ) {
        if self.popup.is_some() {
            return;
        }
        let surface = self
            .surface
            .as_ref()
            .expect("popup surface was not prepared");
        let buffer = self.buffer.as_ref().expect("popup buffer was not prepared");
        let popup = input_method.get_input_popup_surface(surface, qh, ());
        surface.attach(Some(buffer), 0, 0);
        surface.damage_buffer(0, 0, POPUP_WIDTH as i32, POPUP_HEIGHT as i32);
        surface.commit();
        self.popup = Some(popup);
        log("POPUP_SURFACE_CREATED");
    }
}

impl Dispatch<wl_registry::WlRegistry, ()> for State {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = event
        {
            match interface.as_str() {
                "wl_seat" if state.seat.is_none() => {
                    state.seat = Some(registry.bind(name, version.min(9), qh, ()))
                }
                "wl_compositor" if state.compositor.is_none() => {
                    state.compositor = Some(registry.bind(name, version.min(6), qh, ()))
                }
                "wl_shm" if state.shm.is_none() => {
                    state.shm = Some(registry.bind(name, version.min(1), qh, ()))
                }
                "zwp_input_method_manager_v2" if state.manager.is_none() => {
                    state.manager = Some(registry.bind(name, version.min(1), qh, ()))
                }
                _ => {}
            }
        }
    }
}

impl Dispatch<wl_seat::WlSeat, ()> for State {
    fn event(
        _: &mut Self,
        _: &wl_seat::WlSeat,
        _: wl_seat::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

delegate_noop!(State: ignore wl_compositor::WlCompositor);
delegate_noop!(State: ignore wl_surface::WlSurface);
delegate_noop!(State: ignore wl_shm::WlShm);
delegate_noop!(State: ignore wl_shm_pool::WlShmPool);
delegate_noop!(State: ignore wl_buffer::WlBuffer);

impl Dispatch<zwp_input_method_manager_v2::ZwpInputMethodManagerV2, ()> for State {
    fn event(
        _: &mut Self,
        _: &zwp_input_method_manager_v2::ZwpInputMethodManagerV2,
        _: zwp_input_method_manager_v2::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<zwp_input_method_v2::ZwpInputMethodV2, ()> for State {
    fn event(
        state: &mut Self,
        input_method: &zwp_input_method_v2::ZwpInputMethodV2,
        event: zwp_input_method_v2::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        match event {
            zwp_input_method_v2::Event::Activate => {
                state.active = true;
                state.serial = 0;
                state.preedit_sent = false;
                state.commit_sent = false;
                state.create_popup(input_method, qh);
                state.keyboard_grab = Some(input_method.grab_keyboard(qh, ()));
                log("ACTIVE");
            }
            zwp_input_method_v2::Event::Deactivate => {
                state.active = false;
                if let Some(keyboard_grab) = state.keyboard_grab.take() {
                    keyboard_grab.release();
                }
                log("DEACTIVATE");
            }
            zwp_input_method_v2::Event::Done => {
                state.serial = state.serial.wrapping_add(1);
            }
            zwp_input_method_v2::Event::Unavailable => {
                state.active = false;
                log("UNAVAILABLE");
            }
            _ => {}
        }
    }
}

impl Dispatch<zwp_input_method_keyboard_grab_v2::ZwpInputMethodKeyboardGrabV2, ()> for State {
    fn event(
        state: &mut Self,
        _: &zwp_input_method_keyboard_grab_v2::ZwpInputMethodKeyboardGrabV2,
        event: zwp_input_method_keyboard_grab_v2::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let zwp_input_method_keyboard_grab_v2::Event::Key {
            key,
            state: key_state,
            ..
        } = event
        {
            let pressed = key_state.into_result().ok() == Some(wl_keyboard::KeyState::Pressed);
            if pressed {
                log(format!("KEY {key}"));
            }
            if pressed && state.active && !state.preedit_sent && key == 49 {
                if let (Some(input_method), true) = (&state.input_method, state.serial > 0) {
                    input_method.set_preedit_string("nihao".to_owned(), 5, 5);
                    input_method.commit(state.serial);
                    state.preedit_sent = true;
                    log(format!("PREEDIT_SENT serial={}", state.serial));
                }
            } else if key == 57
                && pressed
                && state.active
                && state.preedit_sent
                && !state.commit_sent
            {
                if let Some(input_method) = &state.input_method {
                    input_method.set_preedit_string(String::new(), 0, 0);
                    input_method.commit_string("你好".to_owned());
                    input_method.commit(state.serial);
                    state.commit_sent = true;
                    state.committed += 1;
                    log(format!(
                        "COMMIT_SENT serial={} count={}",
                        state.serial, state.committed
                    ));
                }
            }
        }
    }
}

impl Dispatch<zwp_input_popup_surface_v2::ZwpInputPopupSurfaceV2, ()> for State {
    fn event(
        _: &mut Self,
        _: &zwp_input_popup_surface_v2::ZwpInputPopupSurfaceV2,
        event: zwp_input_popup_surface_v2::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let zwp_input_popup_surface_v2::Event::TextInputRectangle {
            x,
            y,
            width,
            height,
        } = event
        {
            log(format!(
                "POPUP_RECT x={x} y={y} width={width} height={height}"
            ));
        }
    }
}

fn main() {
    let connection = Connection::connect_to_env().expect("connect to private Wayland compositor");
    let mut queue = connection.new_event_queue();
    let qh = queue.handle();
    let display = connection.display();
    let _registry = display.get_registry(&qh, ());
    let mut state = State::default();
    queue.roundtrip(&mut state).expect("read Wayland globals");
    let seat = state
        .seat
        .as_ref()
        .expect("compositor did not advertise wl_seat");
    let compositor = state
        .compositor
        .as_ref()
        .expect("compositor did not advertise wl_compositor");
    let shm = state
        .shm
        .as_ref()
        .expect("compositor did not advertise wl_shm");
    let manager = state
        .manager
        .as_ref()
        .expect("compositor did not advertise input-method-v2");
    let mut pixels = Vec::with_capacity((POPUP_WIDTH * POPUP_HEIGHT * 4) as usize);
    for _ in 0..(POPUP_WIDTH * POPUP_HEIGHT) {
        pixels.extend_from_slice(&[255, 0, 255, 255]);
    }
    let mut file = tempfile::tempfile().expect("create shared-memory popup buffer");
    file.write_all(&pixels).expect("write popup pixels");
    file.flush().expect("flush popup pixels");
    let pool = shm.create_pool(file.as_fd(), pixels.len() as i32, &qh, ());
    state.buffer = Some(pool.create_buffer(
        0,
        POPUP_WIDTH as i32,
        POPUP_HEIGHT as i32,
        (POPUP_WIDTH * 4) as i32,
        wl_shm::Format::Argb8888,
        &qh,
        (),
    ));
    pool.destroy();
    state.surface = Some(compositor.create_surface(&qh, ()));
    state.shm_file = Some(file);
    state.input_method = Some(manager.get_input_method(seat, &qh, ()));
    connection.flush().expect("send get_input_method");
    log("READY");
    loop {
        queue
            .blocking_dispatch(&mut state)
            .expect("dispatch input-method-v2 events");
    }
}
