// Copyright (C) 2026 Jamie Cui
// Author: Jamie Cui
// SPDX-License-Identifier: GPL-3.0-or-later

use super::{
    FontSizeAction, FrameText, OverlayConfig, OverlayState, WHEEL_SCROLL_LINES, adjust_font_size,
    frame_duration,
};
use crate::model::{OverlayCommand, OverlayEvent};
use anyhow::{bail, ensure};
use libc::{c_char, c_double, c_int, c_long, c_ulong, c_void};
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

type Id = *mut c_void;
type Sel = *mut c_void;
type Class = *mut c_void;
type Bool = i8;

const YES: Bool = 1;
const NO: Bool = 0;
const WINDOW_TITLE: &str = "bilive-danmu";
const NS_BORDERLESS_WINDOW_MASK: c_ulong = 0;
const NS_TITLED_WINDOW_MASK: c_ulong = 1 << 0;
const NS_CLOSABLE_WINDOW_MASK: c_ulong = 1 << 1;
const NS_MINIATURIZABLE_WINDOW_MASK: c_ulong = 1 << 2;
const NS_RESIZABLE_WINDOW_MASK: c_ulong = 1 << 3;
const NS_BACKING_STORE_BUFFERED: c_ulong = 2;
const NS_WINDOW_LEVEL_SCREEN_SAVER: c_long = 1000;
const NS_APP_ACTIVATION_POLICY_REGULAR: c_long = 0;
const NS_APP_ACTIVATION_POLICY_ACCESSORY: c_long = 1;
const NS_WINDOW_COLLECTION_BEHAVIOR_CAN_JOIN_ALL_SPACES: c_ulong = 1 << 0;
const NS_WINDOW_COLLECTION_BEHAVIOR_FULL_SCREEN_AUXILIARY: c_ulong = 1 << 8;
const NS_EVENT_TYPE_SCROLL_WHEEL: c_long = 22;
const NS_EVENT_TYPE_KEY_DOWN: c_long = 10;
const NS_EVENT_MODIFIER_FLAG_CONTROL: c_ulong = 1 << 18;

#[repr(C)]
#[derive(Clone, Copy)]
struct NSPoint {
    x: c_double,
    y: c_double,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct NSSize {
    width: c_double,
    height: c_double,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct NSRect {
    origin: NSPoint,
    size: NSSize,
}

#[link(name = "objc")]
unsafe extern "C" {
    fn objc_getClass(name: *const c_char) -> Class;
    fn sel_registerName(name: *const c_char) -> Sel;
    fn objc_msgSend();
    #[cfg(target_arch = "x86_64")]
    fn objc_msgSend_stret();
}

unsafe extern "C" {
    static NSDefaultRunLoopMode: Id;
    static NSFontAttributeName: Id;
    static NSForegroundColorAttributeName: Id;
    static NSStrokeColorAttributeName: Id;
    static NSStrokeWidthAttributeName: Id;
}

macro_rules! msg_send {
    ($receiver:expr, $selector:expr) => {{
        unsafe {
            let function = objc_msgSend as *const ();
            let function: unsafe extern "C" fn(Id, Sel) -> _ = mem::transmute(function);
            function($receiver, $selector)
        }
    }};
    ($receiver:expr, $selector:expr, $a:expr) => {{
        unsafe {
            let function = objc_msgSend as *const ();
            let function: unsafe extern "C" fn(Id, Sel, _) -> _ = mem::transmute(function);
            function($receiver, $selector, $a)
        }
    }};
    ($receiver:expr, $selector:expr, $a:expr, $b:expr) => {{
        unsafe {
            let function = objc_msgSend as *const ();
            let function: unsafe extern "C" fn(Id, Sel, _, _) -> _ = mem::transmute(function);
            function($receiver, $selector, $a, $b)
        }
    }};
    ($receiver:expr, $selector:expr, $a:expr, $b:expr, $c:expr, $d:expr) => {{
        unsafe {
            let function = objc_msgSend as *const ();
            let function: unsafe extern "C" fn(Id, Sel, _, _, _, _) -> _ = mem::transmute(function);
            function($receiver, $selector, $a, $b, $c, $d)
        }
    }};
    ($receiver:expr, $selector:expr $(, $arg:expr)*) => {{ compile_error!("unsupported objc msg_send arity") }};
}

macro_rules! msg_send_class {
    ($receiver:expr, $selector:expr) => {{
        unsafe {
            let function = objc_msgSend as *const ();
            let function: unsafe extern "C" fn(Class, Sel) -> _ = mem::transmute(function);
            function($receiver, $selector)
        }
    }};
    ($receiver:expr, $selector:expr, $a:expr) => {{
        unsafe {
            let function = objc_msgSend as *const ();
            let function: unsafe extern "C" fn(Class, Sel, _) -> _ = mem::transmute(function);
            function($receiver, $selector, $a)
        }
    }};
    ($receiver:expr, $selector:expr, $a:expr, $b:expr) => {{
        unsafe {
            let function = objc_msgSend as *const ();
            let function: unsafe extern "C" fn(Class, Sel, _, _) -> _ = mem::transmute(function);
            function($receiver, $selector, $a, $b)
        }
    }};
    ($receiver:expr, $selector:expr, $a:expr, $b:expr, $c:expr, $d:expr) => {{
        unsafe {
            let function = objc_msgSend as *const ();
            let function: unsafe extern "C" fn(Class, Sel, _, _, _, _) -> _ =
                mem::transmute(function);
            function($receiver, $selector, $a, $b, $c, $d)
        }
    }};
    ($receiver:expr, $selector:expr $(, $arg:expr)*) => {{ compile_error!("unsupported objc class msg_send arity") }};
}

pub fn run(
    mut config: OverlayConfig,
    mut rx: mpsc::UnboundedReceiver<OverlayEvent>,
    mut font_rx: mpsc::UnboundedReceiver<FontSizeAction>,
    command_tx: mpsc::UnboundedSender<OverlayCommand>,
    shutdown: Arc<AtomicBool>,
) -> anyhow::Result<()> {
    unsafe {
        let app = shared_application()?;
        let activation_policy = if config.overlay {
            NS_APP_ACTIVATION_POLICY_ACCESSORY
        } else {
            NS_APP_ACTIVATION_POLICY_REGULAR
        };
        set_activation_policy(app, activation_policy);

        let screen = main_screen()?;
        let frame = screen_frame(screen);
        let width = if config.width == 0 {
            frame.size.width.max(1.0) as u32
        } else {
            config.width
        };
        let height = if config.height == 0 {
            (frame.size.height * config.height_ratio as f64)
                .round()
                .max(config.font_size * 2.0) as u32
        } else {
            config.height
        };
        let rect = NSRect {
            origin: NSPoint {
                x: frame.origin.x + config.x as f64,
                y: frame.origin.y + frame.size.height - height as f64 - config.y as f64,
            },
            size: NSSize {
                width: width as f64,
                height: height as f64,
            },
        };

        let window = create_window(rect, &config)?;
        let view = content_view(window);
        let mut state = OverlayState::new(width, height, &config);
        let mut last_order_front = Instant::now();

        while !shutdown.load(Ordering::Relaxed) {
            drain_events(app, &mut config, &mut state, &command_tx);
            while let Ok(action) = font_rx.try_recv() {
                adjust_font_size(&mut config, &mut state, action);
            }
            while let Ok(event) = rx.try_recv() {
                state.apply_event(event);
            }
            let changed = state.tick();
            if changed {
                draw(view, width, height, &state.frame_texts(), &config)?;
            }
            if config.overlay && last_order_front.elapsed().as_secs() >= 2 {
                order_front(window);
                last_order_front = Instant::now();
            }
            thread::sleep(frame_duration());
        }
    }

    Ok(())
}

unsafe fn shared_application() -> anyhow::Result<Id> {
    let class = class("NSApplication")?;
    Ok(msg_send_class![class, sel("sharedApplication")?])
}

unsafe fn set_activation_policy(app: Id, policy: c_long) {
    let _: Bool = msg_send![app, sel("setActivationPolicy:").unwrap(), policy];
}

unsafe fn main_screen() -> anyhow::Result<Id> {
    let class = class("NSScreen")?;
    let screen: Id = msg_send_class![class, sel("mainScreen")?];
    ensure!(!screen.is_null(), "failed to get main NSScreen");
    Ok(screen)
}

unsafe fn screen_frame(screen: Id) -> NSRect {
    msg_send_rect(screen, sel("frame").unwrap())
}

unsafe fn create_window(rect: NSRect, config: &OverlayConfig) -> anyhow::Result<Id> {
    let class = class("NSWindow")?;
    let window: Id = msg_send_class![class, sel("alloc")?];
    ensure!(!window.is_null(), "failed to allocate NSWindow");
    let style_mask = if config.overlay {
        NS_BORDERLESS_WINDOW_MASK
    } else {
        NS_TITLED_WINDOW_MASK
            | NS_CLOSABLE_WINDOW_MASK
            | NS_MINIATURIZABLE_WINDOW_MASK
            | NS_RESIZABLE_WINDOW_MASK
    };
    let window: Id = msg_send![
        window,
        sel("initWithContentRect:styleMask:backing:defer:")?,
        rect,
        style_mask,
        NS_BACKING_STORE_BUFFERED,
        NO
    ];
    ensure!(!window.is_null(), "failed to initialize NSWindow");

    let _: () = msg_send![window, sel("setOpaque:")?, NO];
    let clear = clear_color()?;
    let _: () = msg_send![window, sel("setBackgroundColor:")?, clear];
    let title = ns_string(WINDOW_TITLE)?;
    let _: () = msg_send![window, sel("setTitle:")?, title];
    if config.overlay {
        let _: () = msg_send![window, sel("setLevel:")?, NS_WINDOW_LEVEL_SCREEN_SAVER];
        let behavior = NS_WINDOW_COLLECTION_BEHAVIOR_CAN_JOIN_ALL_SPACES
            | NS_WINDOW_COLLECTION_BEHAVIOR_FULL_SCREEN_AUXILIARY;
        let _: () = msg_send![window, sel("setCollectionBehavior:")?, behavior];
        let _: () = msg_send![
            window,
            sel("setIgnoresMouseEvents:")?,
            if config.click_through { YES } else { NO }
        ];
        let _: () = msg_send![window, sel("setCanHide:")?, NO];
        let _: () = msg_send![window, sel("orderFrontRegardless")?];
    } else {
        let _: () = msg_send![
            window,
            sel("makeKeyAndOrderFront:")?,
            ptr::null_mut::<c_void>()
        ];
    }
    Ok(window)
}

unsafe fn content_view(window: Id) -> Id {
    msg_send![window, sel("contentView").unwrap()]
}

unsafe fn clear_color() -> anyhow::Result<Id> {
    let class = class("NSColor")?;
    Ok(msg_send_class![class, sel("clearColor")?])
}

unsafe fn order_front(window: Id) {
    let _: () = msg_send![window, sel("orderFrontRegardless").unwrap()];
}

unsafe fn drain_events(
    app: Id,
    config: &mut OverlayConfig,
    state: &mut OverlayState,
    command_tx: &mpsc::UnboundedSender<OverlayCommand>,
) {
    let distant_past: Id = msg_send_class![class("NSDate").unwrap(), sel("distantPast").unwrap()];
    loop {
        let event: Id = msg_send![
            app,
            sel("nextEventMatchingMask:untilDate:inMode:dequeue:").unwrap(),
            c_ulong::MAX,
            distant_past,
            unsafe { NSDefaultRunLoopMode },
            YES
        ];
        if event.is_null() {
            break;
        }
        if handle_scroll_event(event, config, state) {
            continue;
        }
        if handle_key_event(event, command_tx) {
            continue;
        }
        let _: () = msg_send![app, sel("sendEvent:").unwrap(), event];
    }
}

unsafe fn handle_scroll_event(
    event: Id,
    config: &mut OverlayConfig,
    state: &mut OverlayState,
) -> bool {
    let event_type: c_long = msg_send![event, sel("type").unwrap()];
    if event_type != NS_EVENT_TYPE_SCROLL_WHEEL {
        return false;
    }

    let delta_y: c_double = msg_send![event, sel("scrollingDeltaY").unwrap()];
    if delta_y.abs() < f64::EPSILON {
        return true;
    }

    let modifiers: c_ulong = msg_send![event, sel("modifierFlags").unwrap()];
    if modifiers & NS_EVENT_MODIFIER_FLAG_CONTROL != 0 {
        let action = if delta_y.is_sign_positive() {
            FontSizeAction::Increase
        } else {
            FontSizeAction::Decrease
        };
        adjust_font_size(config, state, action);
    } else {
        let lines = if delta_y.is_sign_positive() {
            WHEEL_SCROLL_LINES as isize
        } else {
            -(WHEEL_SCROLL_LINES as isize)
        };
        state.scroll_lines(lines);
    }

    true
}

unsafe fn handle_key_event(event: Id, command_tx: &mpsc::UnboundedSender<OverlayCommand>) -> bool {
    let event_type: c_long = msg_send![event, sel("type").unwrap()];
    if event_type != NS_EVENT_TYPE_KEY_DOWN {
        return false;
    }

    let characters: Id = msg_send![event, sel("charactersIgnoringModifiers").unwrap()];
    if characters.is_null() {
        return false;
    }
    let c_string: *const c_char = msg_send![characters, sel("UTF8String").unwrap()];
    if c_string.is_null() {
        return false;
    }

    let bytes = unsafe { std::ffi::CStr::from_ptr(c_string) }.to_bytes();
    if matches!(bytes, b"r" | b"R") {
        let _ = command_tx.send(OverlayCommand::Reload);
        return true;
    }

    false
}

unsafe fn draw(
    view: Id,
    width: u32,
    height: u32,
    texts: &[FrameText],
    config: &OverlayConfig,
) -> anyhow::Result<()> {
    let image = create_image(width, height)?;
    let _: () = msg_send![image, sel("lockFocus")?];

    let clear = clear_color()?;
    let bounds = NSRect {
        origin: NSPoint { x: 0.0, y: 0.0 },
        size: NSSize {
            width: width as f64,
            height: height as f64,
        },
    };
    let _: () = msg_send![clear, sel("set")?];
    let path = bezier_path_with_rect(bounds)?;
    let _: () = msg_send![path, sel("fill")?];

    for text in texts {
        draw_text(text, height, config)?;
    }

    let _: () = msg_send![image, sel("unlockFocus")?];
    let layer = ensure_layer(view)?;
    let _: () = msg_send![layer, sel("setContents:")?, image];
    Ok(())
}

unsafe fn create_image(width: u32, height: u32) -> anyhow::Result<Id> {
    let class = class("NSImage")?;
    let image: Id = msg_send_class![class, sel("alloc")?];
    let size = NSSize {
        width: width as f64,
        height: height as f64,
    };
    let image: Id = msg_send![image, sel("initWithSize:")?, size];
    ensure!(!image.is_null(), "failed to create NSImage");
    Ok(image)
}

unsafe fn bezier_path_with_rect(rect: NSRect) -> anyhow::Result<Id> {
    let class = class("NSBezierPath")?;
    Ok(msg_send_class![class, sel("bezierPathWithRect:")?, rect])
}

unsafe fn ensure_layer(view: Id) -> anyhow::Result<Id> {
    let _: () = msg_send![view, sel("setWantsLayer:")?, YES];
    let layer: Id = msg_send![view, sel("layer")?];
    ensure!(!layer.is_null(), "failed to get view backing layer");
    Ok(layer)
}

unsafe fn draw_text(text: &FrameText, height: u32, config: &OverlayConfig) -> anyhow::Result<()> {
    let string = ns_string(&text.text)?;
    let attrs = text_attributes(config)?;
    let point = NSPoint {
        x: text.x,
        y: height as f64 - text.y,
    };
    let _: () = msg_send![string, sel("drawAtPoint:withAttributes:")?, point, attrs];
    Ok(())
}

unsafe fn text_attributes(config: &OverlayConfig) -> anyhow::Result<Id> {
    let dict_class = class("NSMutableDictionary")?;
    let attrs: Id = msg_send_class![dict_class, sel("dictionary")?];
    let font_class = class("NSFont")?;
    let family = ns_string(&config.font_family)?;
    let font: Id = msg_send_class![
        font_class,
        sel("fontWithName:size:")?,
        family,
        config.font_size
    ];
    let font = if font.is_null() {
        msg_send_class![font_class, sel("systemFontOfSize:")?, config.font_size]
    } else {
        font
    };
    if !font.is_null() {
        let _: () = msg_send![attrs, sel("setObject:forKey:")?, font, unsafe {
            NSFontAttributeName
        }];
    }
    let color = rgba_color(1.0, 1.0, 1.0, config.opacity)?;
    let _: () = msg_send![attrs, sel("setObject:forKey:")?, color, unsafe {
        NSForegroundColorAttributeName
    }];
    let stroke_color = rgba_color(0.0, 0.0, 0.0, config.opacity)?;
    let _: () = msg_send![attrs, sel("setObject:forKey:")?, stroke_color, unsafe {
        NSStrokeColorAttributeName
    }];
    let stroke_width = ns_number(-4.0)?;
    let _: () = msg_send![attrs, sel("setObject:forKey:")?, stroke_width, unsafe {
        NSStrokeWidthAttributeName
    }];
    Ok(attrs)
}

unsafe fn msg_send_rect(receiver: Id, selector: Sel) -> NSRect {
    #[cfg(target_arch = "x86_64")]
    {
        let function = objc_msgSend_stret as *const ();
        let function: unsafe extern "C" fn(*mut NSRect, Id, Sel) = mem::transmute(function);
        let mut rect = mem::MaybeUninit::<NSRect>::uninit();
        function(rect.as_mut_ptr(), receiver, selector);
        rect.assume_init()
    }

    #[cfg(not(target_arch = "x86_64"))]
    {
        msg_send![receiver, selector]
    }
}

unsafe fn rgba_color(red: f64, green: f64, blue: f64, alpha: f64) -> anyhow::Result<Id> {
    let class = class("NSColor")?;
    Ok(msg_send_class![
        class,
        sel("colorWithCalibratedRed:green:blue:alpha:")?,
        red,
        green,
        blue,
        alpha
    ])
}

unsafe fn ns_number(value: f64) -> anyhow::Result<Id> {
    let class = class("NSNumber")?;
    let number: Id = msg_send_class![class, sel("numberWithDouble:")?, value];
    ensure!(!number.is_null(), "failed to create NSNumber");
    Ok(number)
}

unsafe fn ns_string(value: &str) -> anyhow::Result<Id> {
    let class = class("NSString")?;
    let c_string = CString::new(value)?;
    let string: Id = msg_send_class![class, sel("alloc")?];
    let string: Id = msg_send![string, sel("initWithUTF8String:")?, c_string.as_ptr()];
    ensure!(!string.is_null(), "failed to create NSString");
    Ok(string)
}

unsafe fn class(name: &str) -> anyhow::Result<Class> {
    let c_name = CString::new(name)?;
    let class = objc_getClass(c_name.as_ptr());
    if class.is_null() {
        bail!("Objective-C class {name} not found");
    }
    Ok(class)
}

unsafe fn sel(name: &str) -> anyhow::Result<Sel> {
    let c_name = CString::new(name)?;
    let selector = sel_registerName(c_name.as_ptr());
    if selector.is_null() {
        bail!("Objective-C selector {name} not found");
    }
    Ok(selector)
}
