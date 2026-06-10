// Copyright (C) 2026 Jamie Cui
// Author: Jamie Cui
// SPDX-License-Identifier: GPL-3.0-or-later

use super::{
    FontSizeAction, FrameText, OverlayConfig, OverlayState, WHEEL_SCROLL_LINES, adjust_font_size,
    frame_duration,
};
use crate::model::{OverlayCommand, OverlayEvent};
use anyhow::{Context, anyhow, bail, ensure};
use libc::{c_char, c_double, c_int, c_long, c_uint, c_ulong, c_void};
use std::{
    ffi::CString,
    mem, ptr,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Instant,
};
use tokio::sync::mpsc;
use x11::{
    xlib::{
        self, Above, ButtonPressMask, ClientMessageData, ControlMask, ExposureMask, InputOutput,
        KeyPressMask, PropModeReplace, StructureNotifyMask, SubstructureNotifyMask,
        SubstructureRedirectMask, TrueColor, VisualDepthMask, VisualScreenMask, XA_ATOM,
        XButtonEvent, XClassHint, XClientMessageEvent, XConfigureEvent, XEvent, XKeyEvent,
        XSetWindowAttributes, XVisualInfo,
    },
    xrender::{PictTypeDirect, XRenderFindVisualFormat},
};

const WINDOW_TITLE: &str = "bilive-danmu";
const WINDOW_CLASS: &str = "bilive-danmu";
const XFIXES_SHAPE_INPUT: c_int = 2;
const CAIRO_OPERATOR_CLEAR: c_int = 0;
const CAIRO_OPERATOR_OVER: c_int = 2;
const CLIENT_MESSAGE: c_int = xlib::ClientMessage;
const CONFIGURE_NOTIFY: c_int = xlib::ConfigureNotify;
const DESTROY_NOTIFY: c_int = xlib::DestroyNotify;
const BUTTON_PRESS: c_int = xlib::ButtonPress;
const KEY_PRESS: c_int = xlib::KeyPress;
const WHEEL_UP_BUTTON: c_uint = 4;
const WHEEL_DOWN_BUTTON: c_uint = 5;
const TEXT_OUTLINE_WIDTH: f64 = 2.0;

#[link(name = "Xfixes")]
unsafe extern "C" {
    fn XFixesCreateRegion(
        display: *mut xlib::Display,
        rectangles: *const xlib::XRectangle,
        n_rectangles: c_int,
    ) -> c_ulong;
    fn XFixesDestroyRegion(display: *mut xlib::Display, region: c_ulong);
    fn XFixesSetWindowShapeRegion(
        display: *mut xlib::Display,
        window: xlib::Window,
        shape_kind: c_int,
        x_offset: c_int,
        y_offset: c_int,
        region: c_ulong,
    );
}

#[repr(C)]
struct CairoSurface(c_void);

#[repr(C)]
struct Cairo(c_void);

#[repr(C)]
struct PangoLayout(c_void);

#[repr(C)]
struct PangoFontDescription(c_void);

#[link(name = "cairo")]
unsafe extern "C" {
    fn cairo_xlib_surface_create(
        display: *mut xlib::Display,
        drawable: xlib::Drawable,
        visual: *mut xlib::Visual,
        width: c_int,
        height: c_int,
    ) -> *mut CairoSurface;
    fn cairo_surface_destroy(surface: *mut CairoSurface);
    fn cairo_create(surface: *mut CairoSurface) -> *mut Cairo;
    fn cairo_destroy(cr: *mut Cairo);
    fn cairo_set_operator(cr: *mut Cairo, operator: c_int);
    fn cairo_paint(cr: *mut Cairo);
    fn cairo_set_source_rgba(
        cr: *mut Cairo,
        red: c_double,
        green: c_double,
        blue: c_double,
        alpha: c_double,
    );
    fn cairo_move_to(cr: *mut Cairo, x: c_double, y: c_double);
}

#[link(name = "pango-1.0")]
unsafe extern "C" {
    fn pango_layout_set_text(layout: *mut PangoLayout, text: *const c_char, length: c_int);
    fn pango_layout_set_font_description(
        layout: *mut PangoLayout,
        desc: *const PangoFontDescription,
    );
    fn pango_font_description_from_string(text: *const c_char) -> *mut PangoFontDescription;
    fn pango_font_description_free(desc: *mut PangoFontDescription);
}

#[link(name = "pangocairo-1.0")]
unsafe extern "C" {
    fn pango_cairo_create_layout(cr: *mut Cairo) -> *mut PangoLayout;
    fn pango_cairo_show_layout(cr: *mut Cairo, layout: *mut PangoLayout);
}

#[link(name = "gobject-2.0")]
unsafe extern "C" {
    fn g_object_unref(object: *mut c_void);
}

pub fn run(
    mut config: OverlayConfig,
    mut rx: mpsc::UnboundedReceiver<OverlayEvent>,
    mut font_rx: mpsc::UnboundedReceiver<FontSizeAction>,
    command_tx: mpsc::UnboundedSender<OverlayCommand>,
    shutdown: Arc<AtomicBool>,
) -> anyhow::Result<()> {
    let mut window = X11Overlay::new(&config, command_tx)?;
    let mut state = OverlayState::new(window.width, window.height, &config);
    let mut last_raise = Instant::now();

    loop {
        if shutdown.load(Ordering::Relaxed) {
            break;
        }

        while let Ok(event) = rx.try_recv() {
            state.apply_event(event);
        }
        while let Ok(action) = font_rx.try_recv() {
            adjust_font_size(&mut config, &mut state, action);
        }

        let resized = window.handle_events(&mut state, &mut config)?;
        let changed = state.tick();
        if changed || resized {
            window.draw(&state.frame_texts(), &config)?;
        }

        if config.overlay && last_raise.elapsed().as_secs() >= 2 {
            window.raise();
            last_raise = Instant::now();
        }

        thread::sleep(frame_duration());
    }

    Ok(())
}

struct X11Overlay {
    display: *mut xlib::Display,
    window: xlib::Window,
    visual: *mut xlib::Visual,
    colormap: xlib::Colormap,
    width: u32,
    height: u32,
    wm_delete_window: xlib::Atom,
    command_tx: mpsc::UnboundedSender<OverlayCommand>,
}

impl X11Overlay {
    fn new(
        config: &OverlayConfig,
        command_tx: mpsc::UnboundedSender<OverlayCommand>,
    ) -> anyhow::Result<Self> {
        unsafe {
            let display = xlib::XOpenDisplay(ptr::null());
            if display.is_null() {
                bail!("failed to open X11 display; set DISPLAY or use a supported backend");
            }

            let screen = xlib::XDefaultScreen(display);
            let root = xlib::XRootWindow(display, screen);
            let screen_width = xlib::XDisplayWidth(display, screen).max(1) as u32;
            let screen_height = xlib::XDisplayHeight(display, screen).max(1) as u32;
            let width = if config.width == 0 {
                screen_width
            } else {
                config.width
            };
            let height = if config.height == 0 {
                ((screen_height as f32) * config.height_ratio)
                    .round()
                    .max(config.font_size as f32 * 2.0) as u32
            } else {
                config.height
            };

            let visual_info = find_argb_visual(display, screen)
                .ok_or_else(|| anyhow!("failed to find a 32-bit ARGB X11 visual"))?;
            let visual = visual_info.visual;
            let colormap = xlib::XCreateColormap(display, root, visual, xlib::AllocNone);
            let mut attrs = mem::zeroed::<XSetWindowAttributes>();
            attrs.colormap = colormap;
            attrs.background_pixel = 0;
            attrs.border_pixel = 0;
            attrs.event_mask = ExposureMask | StructureNotifyMask | ButtonPressMask | KeyPressMask;

            let window = xlib::XCreateWindow(
                display,
                root,
                config.x,
                config.y,
                width,
                height,
                0,
                visual_info.depth,
                InputOutput as c_uint,
                visual,
                xlib::CWColormap | xlib::CWBackPixel | xlib::CWBorderPixel | xlib::CWEventMask,
                &mut attrs,
            );
            ensure!(window != 0, "failed to create X11 overlay window");

            set_window_title(display, window)?;
            set_window_class(display, window)?;
            if config.overlay {
                set_no_input_hint(display, window);
                set_window_type(display, window)?;
                set_window_state(display, root, window)?;
                set_motif_borderless(display, window)?;
                if config.click_through {
                    set_click_through(display, window);
                }
            }
            let wm_delete_window = intern_atom(display, "WM_DELETE_WINDOW");
            let mut protocol = wm_delete_window;
            xlib::XSetWMProtocols(display, window, &mut protocol, 1);
            xlib::XMapWindow(display, window);
            if config.overlay {
                xlib::XRaiseWindow(display, window);
            }
            xlib::XFlush(display);

            Ok(Self {
                display,
                window,
                visual,
                colormap,
                width,
                height,
                wm_delete_window,
                command_tx,
            })
        }
    }

    fn handle_events(
        &mut self,
        state: &mut OverlayState,
        config: &mut OverlayConfig,
    ) -> anyhow::Result<bool> {
        let mut resized = false;
        unsafe {
            while xlib::XPending(self.display) > 0 {
                let mut event = mem::zeroed::<XEvent>();
                xlib::XNextEvent(self.display, &mut event);
                match event.get_type() {
                    CONFIGURE_NOTIFY => {
                        let configure: XConfigureEvent = event.into();
                        let width = configure.width.max(1) as u32;
                        let height = configure.height.max(1) as u32;
                        if width != self.width || height != self.height {
                            self.width = width;
                            self.height = height;
                            state.resize(width, height, config);
                            resized = true;
                        }
                    }
                    BUTTON_PRESS => {
                        let button: XButtonEvent = event.into();
                        handle_button_press(&button, config, state);
                    }
                    KEY_PRESS => {
                        let mut key: XKeyEvent = event.into();
                        handle_key_press(&mut key, &self.command_tx);
                    }
                    CLIENT_MESSAGE => {
                        let message: XClientMessageEvent = event.into();
                        if message.data.get_long(0) as xlib::Atom == self.wm_delete_window {
                            bail!("overlay window was closed");
                        }
                    }
                    DESTROY_NOTIFY => bail!("overlay window was destroyed"),
                    _ => {}
                }
            }
        }
        Ok(resized)
    }

    fn draw(&mut self, texts: &[FrameText], config: &OverlayConfig) -> anyhow::Result<()> {
        unsafe {
            let surface = cairo_xlib_surface_create(
                self.display,
                self.window,
                self.visual,
                self.width as c_int,
                self.height as c_int,
            );
            ensure!(!surface.is_null(), "failed to create Cairo Xlib surface");
            let cr = cairo_create(surface);
            if cr.is_null() {
                cairo_surface_destroy(surface);
                bail!("failed to create Cairo context");
            }

            cairo_set_operator(cr, CAIRO_OPERATOR_CLEAR);
            cairo_paint(cr);
            cairo_set_operator(cr, CAIRO_OPERATOR_OVER);

            for text in texts {
                draw_text(cr, text, config)?;
            }

            cairo_destroy(cr);
            cairo_surface_destroy(surface);
            xlib::XFlush(self.display);
        }
        Ok(())
    }

    fn raise(&self) {
        unsafe {
            let mut changes = mem::zeroed::<xlib::XWindowChanges>();
            changes.stack_mode = Above;
            xlib::XConfigureWindow(
                self.display,
                self.window,
                xlib::CWStackMode.into(),
                &mut changes,
            );
            xlib::XRaiseWindow(self.display, self.window);
            xlib::XFlush(self.display);
        }
    }
}

impl Drop for X11Overlay {
    fn drop(&mut self) {
        unsafe {
            xlib::XDestroyWindow(self.display, self.window);
            xlib::XFreeColormap(self.display, self.colormap);
            xlib::XCloseDisplay(self.display);
        }
    }
}

fn handle_button_press(event: &XButtonEvent, config: &mut OverlayConfig, state: &mut OverlayState) {
    let action = match event.button {
        WHEEL_UP_BUTTON => FontSizeAction::Increase,
        WHEEL_DOWN_BUTTON => FontSizeAction::Decrease,
        _ => return,
    };

    if event.state & ControlMask != 0 {
        adjust_font_size(config, state, action);
        return;
    }

    let lines = match event.button {
        WHEEL_UP_BUTTON => WHEEL_SCROLL_LINES as isize,
        WHEEL_DOWN_BUTTON => -(WHEEL_SCROLL_LINES as isize),
        _ => return,
    };
    state.scroll_lines(lines);
}

fn handle_key_press(event: &mut XKeyEvent, command_tx: &mpsc::UnboundedSender<OverlayCommand>) {
    let mut buffer = [0 as c_char; 8];
    let mut keysym = 0;
    let len = unsafe {
        xlib::XLookupString(
            event,
            buffer.as_mut_ptr(),
            buffer.len() as c_int,
            &mut keysym,
            ptr::null_mut(),
        )
    };
    if len != 1 {
        return;
    }

    let key = buffer[0] as u8;
    if key == b'r' || key == b'R' {
        let _ = command_tx.send(OverlayCommand::Reload);
    }
}

fn draw_text(cr: *mut Cairo, text: &FrameText, config: &OverlayConfig) -> anyhow::Result<()> {
    let font = CString::new(format!(
        "{} {}",
        config.font_family,
        config.font_size.round()
    ))
    .context("font description contains NUL byte")?;
    let value = CString::new(text.text.as_str()).context("danmu text contains NUL byte")?;

    unsafe {
        let desc = pango_font_description_from_string(font.as_ptr());
        ensure!(!desc.is_null(), "failed to create Pango font description");
        let layout = pango_cairo_create_layout(cr);
        if layout.is_null() {
            pango_font_description_free(desc);
            bail!("failed to create Pango layout");
        }

        pango_layout_set_font_description(layout, desc);
        pango_layout_set_text(layout, value.as_ptr(), -1);

        let x = text.x;
        let y = text.y - config.font_size;
        for (dx, dy) in outline_offsets(TEXT_OUTLINE_WIDTH) {
            cairo_move_to(cr, x + dx, y + dy);
            cairo_set_source_rgba(cr, 0.0, 0.0, 0.0, config.opacity);
            pango_cairo_show_layout(cr, layout);
        }
        cairo_move_to(cr, x, y);
        cairo_set_source_rgba(cr, 1.0, 1.0, 1.0, config.opacity);
        pango_cairo_show_layout(cr, layout);

        g_object_unref(layout.cast());
        pango_font_description_free(desc);
    }
    Ok(())
}

fn outline_offsets(width: f64) -> [(f64, f64); 8] {
    [
        (-width, -width),
        (0.0, -width),
        (width, -width),
        (-width, 0.0),
        (width, 0.0),
        (-width, width),
        (0.0, width),
        (width, width),
    ]
}

unsafe fn find_argb_visual(display: *mut xlib::Display, screen: c_int) -> Option<XVisualInfo> {
    let mut template = unsafe { mem::zeroed::<XVisualInfo>() };
    template.screen = screen;
    template.depth = 32;
    template.class = TrueColor;
    let mut count = 0;
    let infos = unsafe {
        xlib::XGetVisualInfo(
            display,
            VisualScreenMask | VisualDepthMask | xlib::VisualClassMask,
            &mut template,
            &mut count,
        )
    };
    if infos.is_null() {
        return None;
    }

    let mut selected = None;
    for index in 0..count {
        let info = unsafe { *infos.add(index as usize) };
        let format = unsafe { XRenderFindVisualFormat(display, info.visual) };
        if format.is_null() {
            continue;
        }
        let format = unsafe { &*format };
        if format.type_ == PictTypeDirect && format.depth == 32 && format.direct.alphaMask > 0 {
            selected = Some(info);
            break;
        }
    }
    unsafe {
        xlib::XFree(infos.cast());
    }
    selected
}

fn set_window_title(display: *mut xlib::Display, window: xlib::Window) -> anyhow::Result<()> {
    let title = CString::new(WINDOW_TITLE)?;
    unsafe {
        xlib::XStoreName(display, window, title.as_ptr());
    }
    Ok(())
}

fn set_window_class(display: *mut xlib::Display, window: xlib::Window) -> anyhow::Result<()> {
    let name = CString::new(WINDOW_CLASS)?;
    let class = CString::new(WINDOW_CLASS)?;
    unsafe {
        let mut hint = XClassHint {
            res_name: name.as_ptr() as *mut c_char,
            res_class: class.as_ptr() as *mut c_char,
        };
        xlib::XSetClassHint(display, window, &mut hint);
    }
    Ok(())
}

fn set_no_input_hint(display: *mut xlib::Display, window: xlib::Window) {
    unsafe {
        let mut hints = mem::zeroed::<xlib::XWMHints>();
        hints.flags = xlib::InputHint;
        hints.input = 0;
        xlib::XSetWMHints(display, window, &mut hints);
    }
}

fn set_window_type(display: *mut xlib::Display, window: xlib::Window) -> anyhow::Result<()> {
    let atom_type = unsafe { intern_atom(display, "_NET_WM_WINDOW_TYPE") };
    let dock = unsafe { intern_atom(display, "_NET_WM_WINDOW_TYPE_DOCK") };
    let atoms = [dock];
    unsafe {
        xlib::XChangeProperty(
            display,
            window,
            atom_type,
            XA_ATOM,
            32,
            PropModeReplace,
            atoms.as_ptr().cast::<u8>(),
            atoms.len() as c_int,
        );
    }
    Ok(())
}

fn set_window_state(
    display: *mut xlib::Display,
    root: xlib::Window,
    window: xlib::Window,
) -> anyhow::Result<()> {
    let state = unsafe { intern_atom(display, "_NET_WM_STATE") };
    let atoms = [
        unsafe { intern_atom(display, "_NET_WM_STATE_ABOVE") },
        unsafe { intern_atom(display, "_NET_WM_STATE_STICKY") },
        unsafe { intern_atom(display, "_NET_WM_STATE_SKIP_TASKBAR") },
        unsafe { intern_atom(display, "_NET_WM_STATE_SKIP_PAGER") },
    ];
    unsafe {
        xlib::XChangeProperty(
            display,
            window,
            state,
            XA_ATOM,
            32,
            PropModeReplace,
            atoms.as_ptr().cast::<u8>(),
            atoms.len() as c_int,
        );

        for atom in atoms {
            let mut data = ClientMessageData::new();
            data.set_long(0, 1);
            data.set_long(1, atom as c_long);
            data.set_long(2, 0);
            data.set_long(3, 1);
            let mut event = XClientMessageEvent {
                type_: CLIENT_MESSAGE,
                serial: 0,
                send_event: 1,
                display,
                window,
                message_type: state,
                format: 32,
                data,
            };
            xlib::XSendEvent(
                display,
                root,
                0,
                SubstructureRedirectMask | SubstructureNotifyMask,
                (&mut event as *mut XClientMessageEvent).cast::<XEvent>(),
            );
        }
    }
    Ok(())
}

fn set_motif_borderless(display: *mut xlib::Display, window: xlib::Window) -> anyhow::Result<()> {
    let property = unsafe { intern_atom(display, "_MOTIF_WM_HINTS") };
    let hints: [c_ulong; 5] = [2, 0, 0, 0, 0];
    unsafe {
        xlib::XChangeProperty(
            display,
            window,
            property,
            property,
            32,
            PropModeReplace,
            hints.as_ptr().cast::<u8>(),
            hints.len() as c_int,
        );
    }
    Ok(())
}

fn set_click_through(display: *mut xlib::Display, window: xlib::Window) {
    unsafe {
        let region = XFixesCreateRegion(display, ptr::null(), 0);
        XFixesSetWindowShapeRegion(display, window, XFIXES_SHAPE_INPUT, 0, 0, region);
        XFixesDestroyRegion(display, region);
    }
}

unsafe fn intern_atom(display: *mut xlib::Display, name: &str) -> xlib::Atom {
    let name = CString::new(name).expect("atom name must not contain NUL");
    unsafe { xlib::XInternAtom(display, name.as_ptr(), 0) }
}
