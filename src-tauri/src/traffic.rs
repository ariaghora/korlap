// macOS traffic light positioning.
// Adapted from db-atelier's approach: moves the button group superview
// as a unit, preserving native inter-button spacing.

use cocoa::appkit::{NSView, NSWindow, NSWindowButton};
use cocoa::base::id;
use cocoa::foundation::NSRect;
use objc::{msg_send, sel, sel_impl};
use rand::distributions::Alphanumeric;
use rand::Rng;
use std::ffi::c_void;
use tauri::{Runtime, WebviewWindow};

pub struct UnsafeWindowHandle(pub *mut c_void);
unsafe impl Send for UnsafeWindowHandle {}
unsafe impl Sync for UnsafeWindowHandle {}

pub fn position_traffic_lights(ns_window_handle: UnsafeWindowHandle, x: f64, y: f64) {
    let ns_window = ns_window_handle.0 as id;
    unsafe {
        let close = ns_window.standardWindowButton_(NSWindowButton::NSWindowCloseButton);
        if close.is_null() {
            return;
        }

        let button_group = close.superview();
        let title_bar_container = button_group.superview();

        let close_rect: NSRect = msg_send![close, frame];
        let button_height = close_rect.size.height;

        let title_bar_frame_height = button_height + y;
        let mut title_bar_rect = NSView::frame(title_bar_container);
        title_bar_rect.size.height = title_bar_frame_height;
        title_bar_rect.origin.y =
            NSView::frame(ns_window).size.height - title_bar_frame_height;
        let _: () = msg_send![title_bar_container, setFrame: title_bar_rect];

        let mut group_rect: NSRect = NSView::frame(button_group);
        group_rect.origin.x = x;
        group_rect.origin.y = (title_bar_frame_height - group_rect.size.height) / 2.0;
        let _: () = msg_send![button_group, setFrame: group_rect];
    }
}

#[derive(Debug)]
struct WindowState<R: Runtime> {
    window: WebviewWindow<R>,
}

fn with_window_state<R: Runtime, F: FnOnce(&mut WindowState<R>) -> T, T>(
    this: &objc::runtime::Object,
    func: F,
) {
    let ptr = unsafe {
        let x: *mut c_void = *this.get_ivar("app_box");
        &mut *(x as *mut WindowState<R>)
    };
    func(ptr);
}

pub fn setup_traffic_light_positioner<R: Runtime>(window: WebviewWindow<R>, x: f64, y: f64) {
    use cocoa::appkit::NSWindow;
    use cocoa::base::BOOL;
    use cocoa::foundation::NSUInteger;
    use objc::runtime::{Object, Sel};

    position_traffic_lights(
        UnsafeWindowHandle(window.ns_window().expect("Failed to get window handle")),
        x,
        y,
    );

    unsafe {
        let ns_win = window
            .ns_window()
            .expect("NS Window should exist to mount traffic light delegate.")
            as id;

        let current_delegate: id = ns_win.delegate();

        extern "C" fn on_window_should_close(this: &Object, _cmd: Sel, sender: id) -> BOOL {
            unsafe {
                let super_del: id = *this.get_ivar("super_delegate");
                msg_send![super_del, windowShouldClose: sender]
            }
        }

        extern "C" fn on_window_will_close(this: &Object, _cmd: Sel, notification: id) {
            unsafe {
                let super_del: id = *this.get_ivar("super_delegate");
                let _: () = msg_send![super_del, windowWillClose: notification];
            }
        }

        extern "C" fn on_window_did_resize<R: Runtime>(
            this: &Object,
            _cmd: Sel,
            notification: id,
        ) {
            unsafe {
                let pad_x: f64 = *this.get_ivar("pad_x");
                let pad_y: f64 = *this.get_ivar("pad_y");

                with_window_state(&*this, |state: &mut WindowState<R>| {
                    let id = state
                        .window
                        .ns_window()
                        .expect("NS window should exist on resize")
                        as id;
                    position_traffic_lights(
                        UnsafeWindowHandle(id as *mut c_void),
                        pad_x,
                        pad_y,
                    );
                });

                let super_del: id = *this.get_ivar("super_delegate");
                let _: () = msg_send![super_del, windowDidResize: notification];
            }
        }

        extern "C" fn on_window_did_move(this: &Object, _cmd: Sel, notification: id) {
            unsafe {
                let super_del: id = *this.get_ivar("super_delegate");
                let _: () = msg_send![super_del, windowDidMove: notification];
            }
        }

        extern "C" fn on_window_did_change_backing_properties(
            this: &Object,
            _cmd: Sel,
            notification: id,
        ) {
            unsafe {
                let super_del: id = *this.get_ivar("super_delegate");
                let _: () =
                    msg_send![super_del, windowDidChangeBackingProperties: notification];
            }
        }

        extern "C" fn on_window_did_become_key(this: &Object, _cmd: Sel, notification: id) {
            unsafe {
                let super_del: id = *this.get_ivar("super_delegate");
                let _: () = msg_send![super_del, windowDidBecomeKey: notification];
            }
        }

        extern "C" fn on_window_did_resign_key(this: &Object, _cmd: Sel, notification: id) {
            unsafe {
                let super_del: id = *this.get_ivar("super_delegate");
                let _: () = msg_send![super_del, windowDidResignKey: notification];
            }
        }

        extern "C" fn on_dragging_entered(this: &Object, _cmd: Sel, notification: id) -> BOOL {
            unsafe {
                let super_del: id = *this.get_ivar("super_delegate");
                msg_send![super_del, draggingEntered: notification]
            }
        }

        extern "C" fn on_prepare_for_drag_operation(
            this: &Object,
            _cmd: Sel,
            notification: id,
        ) -> BOOL {
            unsafe {
                let super_del: id = *this.get_ivar("super_delegate");
                msg_send![super_del, prepareForDragOperation: notification]
            }
        }

        extern "C" fn on_perform_drag_operation(this: &Object, _cmd: Sel, sender: id) -> BOOL {
            unsafe {
                let super_del: id = *this.get_ivar("super_delegate");
                msg_send![super_del, performDragOperation: sender]
            }
        }

        extern "C" fn on_conclude_drag_operation(this: &Object, _cmd: Sel, notification: id) {
            unsafe {
                let super_del: id = *this.get_ivar("super_delegate");
                let _: () = msg_send![super_del, concludeDragOperation: notification];
            }
        }

        extern "C" fn on_dragging_exited(this: &Object, _cmd: Sel, notification: id) {
            unsafe {
                let super_del: id = *this.get_ivar("super_delegate");
                let _: () = msg_send![super_del, draggingExited: notification];
            }
        }

        extern "C" fn on_window_will_use_full_screen_presentation_options(
            this: &Object,
            _cmd: Sel,
            window: id,
            proposed_options: NSUInteger,
        ) -> NSUInteger {
            unsafe {
                let super_del: id = *this.get_ivar("super_delegate");
                msg_send![super_del, window: window willUseFullScreenPresentationOptions: proposed_options]
            }
        }

        extern "C" fn on_window_did_enter_full_screen(
            this: &Object,
            _cmd: Sel,
            notification: id,
        ) {
            unsafe {
                let super_del: id = *this.get_ivar("super_delegate");
                let _: () = msg_send![super_del, windowDidEnterFullScreen: notification];
            }
        }

        extern "C" fn on_window_will_enter_full_screen(
            this: &Object,
            _cmd: Sel,
            notification: id,
        ) {
            unsafe {
                let super_del: id = *this.get_ivar("super_delegate");
                let _: () = msg_send![super_del, windowWillEnterFullScreen: notification];
            }
        }

        extern "C" fn on_window_did_exit_full_screen<R: Runtime>(
            this: &Object,
            _cmd: Sel,
            notification: id,
        ) {
            unsafe {
                let pad_x: f64 = *this.get_ivar("pad_x");
                let pad_y: f64 = *this.get_ivar("pad_y");

                with_window_state(&*this, |state: &mut WindowState<R>| {
                    let id = state.window.ns_window().expect("Failed to get NS window")
                        as id;
                    position_traffic_lights(
                        UnsafeWindowHandle(id as *mut c_void),
                        pad_x,
                        pad_y,
                    );
                });

                let super_del: id = *this.get_ivar("super_delegate");
                let _: () = msg_send![super_del, windowDidExitFullScreen: notification];
            }
        }

        extern "C" fn on_window_will_exit_full_screen(
            this: &Object,
            _cmd: Sel,
            notification: id,
        ) {
            unsafe {
                let super_del: id = *this.get_ivar("super_delegate");
                let _: () = msg_send![super_del, windowWillExitFullScreen: notification];
            }
        }

        extern "C" fn on_window_did_fail_to_enter_full_screen(
            this: &Object,
            _cmd: Sel,
            window: id,
        ) {
            unsafe {
                let super_del: id = *this.get_ivar("super_delegate");
                let _: () = msg_send![super_del, windowDidFailToEnterFullScreen: window];
            }
        }

        extern "C" fn on_effective_appearance_did_change(
            this: &Object,
            _cmd: Sel,
            notification: id,
        ) {
            unsafe {
                let super_del: id = *this.get_ivar("super_delegate");
                let _: () =
                    msg_send![super_del, effectiveAppearanceDidChange: notification];
            }
        }

        extern "C" fn on_effective_appearance_did_changed_on_main_thread(
            this: &Object,
            _cmd: Sel,
            notification: id,
        ) {
            unsafe {
                let super_del: id = *this.get_ivar("super_delegate");
                let _: () = msg_send![
                    super_del,
                    effectiveAppearanceDidChangedOnMainThread: notification
                ];
            }
        }

        let window_label = window.label().to_string();
        let app_state = WindowState { window };
        let app_box = Box::into_raw(Box::new(app_state)) as *mut c_void;

        let random_str: String = rand::thread_rng()
            .sample_iter(&Alphanumeric)
            .take(20)
            .map(char::from)
            .collect();

        let delegate_name = format!("windowDelegate_{}_{}", window_label, random_str);

        ns_win.setDelegate_(cocoa::delegate!(&delegate_name, {
            window: id = ns_win,
            app_box: *mut c_void = app_box,
            super_delegate: id = current_delegate,
            pad_x: f64 = x,
            pad_y: f64 = y,
            (windowShouldClose:) => on_window_should_close as extern fn(&Object, Sel, id) -> BOOL,
            (windowWillClose:) => on_window_will_close as extern fn(&Object, Sel, id),
            (windowDidResize:) => on_window_did_resize::<R> as extern fn(&Object, Sel, id),
            (windowDidMove:) => on_window_did_move as extern fn(&Object, Sel, id),
            (windowDidChangeBackingProperties:) => on_window_did_change_backing_properties as extern fn(&Object, Sel, id),
            (windowDidBecomeKey:) => on_window_did_become_key as extern fn(&Object, Sel, id),
            (windowDidResignKey:) => on_window_did_resign_key as extern fn(&Object, Sel, id),
            (draggingEntered:) => on_dragging_entered as extern fn(&Object, Sel, id) -> BOOL,
            (prepareForDragOperation:) => on_prepare_for_drag_operation as extern fn(&Object, Sel, id) -> BOOL,
            (performDragOperation:) => on_perform_drag_operation as extern fn(&Object, Sel, id) -> BOOL,
            (concludeDragOperation:) => on_conclude_drag_operation as extern fn(&Object, Sel, id),
            (draggingExited:) => on_dragging_exited as extern fn(&Object, Sel, id),
            (window:willUseFullScreenPresentationOptions:) => on_window_will_use_full_screen_presentation_options as extern fn(&Object, Sel, id, NSUInteger) -> NSUInteger,
            (windowDidEnterFullScreen:) => on_window_did_enter_full_screen as extern fn(&Object, Sel, id),
            (windowWillEnterFullScreen:) => on_window_will_enter_full_screen as extern fn(&Object, Sel, id),
            (windowDidExitFullScreen:) => on_window_did_exit_full_screen::<R> as extern fn(&Object, Sel, id),
            (windowWillExitFullScreen:) => on_window_will_exit_full_screen as extern fn(&Object, Sel, id),
            (windowDidFailToEnterFullScreen:) => on_window_did_fail_to_enter_full_screen as extern fn(&Object, Sel, id),
            (effectiveAppearanceDidChange:) => on_effective_appearance_did_change as extern fn(&Object, Sel, id),
            (effectiveAppearanceDidChangedOnMainThread:) => on_effective_appearance_did_changed_on_main_thread as extern fn(&Object, Sel, id)
        }));
    }
}
