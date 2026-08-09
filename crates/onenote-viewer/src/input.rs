use crate::navigation_history::HistoryDirection;
use gtk::glib;
use gtk::prelude::*;

pub(crate) fn history_mouse_controller(
    navigate: impl Fn(HistoryDirection) + 'static,
) -> gtk::EventControllerLegacy {
    let controller = gtk::EventControllerLegacy::new();
    controller.set_propagation_phase(gtk::PropagationPhase::Capture);
    controller.connect_event(move |_, event| {
        let Some(button_event) = event.downcast_ref::<gtk::gdk::ButtonEvent>() else {
            return glib::Propagation::Proceed;
        };
        let Some(direction) = history_direction_for_mouse_button(button_event.button()) else {
            return glib::Propagation::Proceed;
        };
        if event.event_type() == gtk::gdk::EventType::ButtonPress {
            navigate(direction);
        }
        // Consume both halves of a recognized history-button click. All
        // unrelated events proceed unchanged and never enter gesture state.
        glib::Propagation::Stop
    });
    controller
}

pub(crate) fn focus_initial_navigation(window: &gtk::Window, navigation: &gtk::ListView) {
    gtk::prelude::GtkWindowExt::set_focus(window, Some(navigation));
}

fn history_direction_for_mouse_button(button: u32) -> Option<HistoryDirection> {
    // Linux exposes both generic side buttons and semantic navigation
    // buttons. GDK preserves their conventional 8-11 numbering on X11
    // and Wayland.
    match button {
        8 | 11 => Some(HistoryDirection::Back),
        9 | 10 => Some(HistoryDirection::Forward),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};
    use std::process::Command;
    use std::rc::Rc;

    #[test]
    fn classifies_conventional_and_semantic_navigation_buttons() {
        for button in [8, 11] {
            assert_eq!(
                history_direction_for_mouse_button(button),
                Some(HistoryDirection::Back)
            );
        }
        for button in [9, 10] {
            assert_eq!(
                history_direction_for_mouse_button(button),
                Some(HistoryDirection::Forward)
            );
        }
        for button in [1, 2, 3, 4, 5, 6, 7, 12] {
            assert_eq!(history_direction_for_mouse_button(button), None);
        }
    }

    #[test]
    fn mouse_history_preserves_primary_clicks_and_handles_navigation_buttons() {
        crate::test_support::run_gtk_test(mouse_history_preserves_primary_clicks_gtk);
    }

    fn mouse_history_preserves_primary_clicks_gtk() {
        if !x11_pointer_injection_available() {
            return;
        }

        let title = format!("onenote-viewer-mouse-history-{}", std::process::id());
        let primary_clicks = Rc::new(Cell::new(0_u32));
        let target = gtk::Button::with_label("Target");
        let callback_clicks = Rc::clone(&primary_clicks);
        target.connect_clicked(move |_| callback_clicks.set(callback_clicks.get() + 1));

        let navigations = Rc::new(RefCell::new(Vec::new()));
        let callback_navigations = Rc::clone(&navigations);
        let controller = history_mouse_controller(move |direction| {
            callback_navigations.borrow_mut().push(direction);
        });
        assert_eq!(
            controller.propagation_phase(),
            gtk::PropagationPhase::Capture
        );
        let window = gtk::Window::builder()
            .title(&title)
            .default_width(180)
            .default_height(100)
            .child(&target)
            .build();
        window.add_controller(controller);
        window.present();
        drain_gtk_events();

        let window_id = xdotool_window_id(&title);
        xdotool_click(&window_id, 1);
        drain_gtk_events();
        assert_eq!(primary_clicks.get(), 1);
        assert!(navigations.borrow().is_empty());

        // Xvfb's synthetic core pointer rejects button 11 at the XTEST
        // protocol boundary. Its mapping remains covered by the classifier
        // test; exercise every extra button Xvfb can inject here.
        for button in [8, 9, 10] {
            xdotool_click(&window_id, button);
            drain_gtk_events();
        }
        assert_eq!(
            *navigations.borrow(),
            [
                HistoryDirection::Back,
                HistoryDirection::Forward,
                HistoryDirection::Forward,
            ]
        );
        assert_eq!(primary_clicks.get(), 1);
        let status = Command::new("xdotool")
            .args(["mousemove", "0", "0"])
            .status()
            .expect("move pointer away from test window");
        assert!(status.success(), "xdotool failed to move the pointer");
        drain_gtk_events();
        window.close();
        drain_gtk_events();
    }

    #[test]
    fn initial_focus_uses_navigation_instead_of_close_notebook() {
        crate::test_support::run_gtk_test(initial_focus_uses_navigation_gtk);
    }

    fn initial_focus_uses_navigation_gtk() {
        let close_source = gtk::Button::with_label("Close selected notebook");
        let navigation =
            gtk::ListView::new(None::<gtk::SelectionModel>, None::<gtk::ListItemFactory>);
        let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
        content.append(&close_source);
        content.append(&navigation);
        let window = gtk::Window::builder().child(&content).build();
        window.present();
        focus_initial_navigation(&window, &navigation);
        drain_gtk_events();

        let expected: gtk::Widget = navigation.clone().upcast();
        assert_eq!(
            gtk::prelude::GtkWindowExt::focus(&window).as_ref(),
            Some(&expected)
        );
        window.close();
    }

    fn x11_pointer_injection_available() -> bool {
        let Some(display) = gtk::gdk::Display::default() else {
            return false;
        };
        if !display.type_().name().contains("X11") {
            return false;
        }
        Command::new("xdotool")
            .arg("--version")
            .output()
            .is_ok_and(|output| output.status.success())
    }

    fn xdotool_window_id(title: &str) -> String {
        let output = Command::new("xdotool")
            .args(["search", "--onlyvisible", "--name", title])
            .output()
            .expect("run xdotool window search");
        assert!(
            output.status.success(),
            "xdotool could not find test window: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout)
            .expect("xdotool window id is UTF-8")
            .lines()
            .next()
            .expect("xdotool returned a window id")
            .to_owned()
    }

    fn xdotool_click(window_id: &str, button: u32) {
        let button = button.to_string();
        let status = Command::new("xdotool")
            .args([
                "mousemove",
                "--window",
                window_id,
                "60",
                "40",
                "click",
                &button,
            ])
            .status()
            .expect("run xdotool click");
        assert!(status.success(), "xdotool failed to inject button {button}");
    }

    fn drain_gtk_events() {
        while glib::MainContext::default().iteration(false) {}
    }
}
