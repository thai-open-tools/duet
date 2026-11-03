//! Following a held scrollbar or strip across the whole window, for every view's bars.

use gpui_kit::{
    Context, DispatchPhase, MouseButton, MouseMoveEvent, MouseUpEvent, Pixels, Point, WeakEntity,
    Window,
};

/// While a bar's drag is held, the pointer is followed anywhere in the window, as gpui-kit's
/// vertical scrollbar does: `moved` gets each position while the left button is down, and
/// `end` runs when it comes up or a move arrives without it. Only held moves are kept from the
/// elements under the pointer (so a drag never extends a text selection); a plain move reaches
/// them, such as a tooltip's.
pub(crate) fn follow_drag<V: 'static>(
    view: WeakEntity<V>,
    window: &mut Window,
    moved: fn(&mut V, Point<Pixels>, &mut Context<V>),
    end: fn(&mut V, &mut Context<V>),
) {
    let on_move = view.clone();
    window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
        if phase != DispatchPhase::Bubble {
            return;
        }
        let held = event.pressed_button == Some(MouseButton::Left);
        if held {
            cx.stop_propagation();
        }
        let _ = on_move.update(cx, |this, cx| {
            if held {
                moved(this, event.position, cx);
            } else {
                end(this, cx);
            }
        });
    });
    window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
        if phase == DispatchPhase::Bubble && event.button == MouseButton::Left {
            let _ = view.update(cx, end);
        }
    });
}
