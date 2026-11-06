//! The confirmation dialog. Every action that changes packages asks first, showing what it
//! will do. Cancel has the focus, so Enter or Escape cancels.

use super::*;
use gpui_kit::component::button::{Button, ButtonVariants};

/// The work to run once the user presses Yes.
pub(crate) type Run = Box<dyn FnOnce(&mut AppView, &mut Context<AppView>)>;

pub(crate) struct Confirm {
    pub title: String,
    /// One line per package or file the action touches.
    pub lines: Vec<String>,
    /// An extra sentence about what the action does, shown above the buttons.
    pub note: Option<String>,
    pub yes: String,
    /// Remove, reinstall and delete use the danger style for Yes.
    pub danger: bool,
    pub run: Run,
}

impl AppView {
    pub(crate) fn ask(&mut self, confirm: Confirm, cx: &mut Context<Self>) {
        self.confirm = Some(confirm);
        self.focus_confirm = true;
        self.menu_for = None;
        cx.notify();
    }

    pub(crate) fn cancel_confirm(&mut self, cx: &mut Context<Self>) {
        if self.confirm.take().is_some() {
            self.status = "Cancelled.".into();
        }
        cx.notify();
    }

    pub(crate) fn confirm_yes(&mut self, cx: &mut Context<Self>) {
        if let Some(confirm) = self.confirm.take() {
            (confirm.run)(self, cx);
        }
        cx.notify();
    }

    pub(crate) fn render_confirm(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let confirm = self.confirm.as_ref()?;
        // A long batch lists its first lines, then how many are left, so the dialog stays short.
        const SHOWN: usize = 20;
        let mut lines: Vec<AnyElement> = confirm
            .lines
            .iter()
            .take(SHOWN)
            .map(|line| div().text_color(rgb(TEXT)).child(line.clone()).into_any_element())
            .collect();
        if confirm.lines.len() > SHOWN {
            let more = confirm.lines.len() - SHOWN;
            lines.push(div().text_color(rgb(MUTED)).child(format!("…and {more} more")).into_any_element());
        }
        let yes = if confirm.danger {
            Button::new("confirm-yes").danger()
        } else {
            Button::new("confirm-yes").primary()
        };
        Some(
            div()
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .bg(rgba(SCRIM))
                .track_focus(&self.confirm_focus)
                .on_key_down(cx.listener(|this: &mut AppView, ev: &KeyDownEvent, _window, cx| {
                    if ev.keystroke.key == "escape" || ev.keystroke.key == "enter" {
                        this.cancel_confirm(cx);
                    }
                }))
                .child(
                    div()
                        .w(px(440.))
                        .p_4()
                        .gap_3()
                        .flex()
                        .flex_col()
                        .rounded(px(8.))
                        .bg(rgb(PANEL))
                        .border_1()
                        .border_color(rgb(BORDER))
                        .child(div().text_lg().child(confirm.title.clone()))
                        .child(div().flex().flex_col().gap_1().children(lines))
                        .when_some(confirm.note.clone(), |d, note| {
                            d.child(div().text_color(rgb(MUTED)).child(note))
                        })
                        .child(
                            div()
                                .flex()
                                .justify_end()
                                .gap_2()
                                .child(
                                    Button::new("confirm-cancel")
                                        .ghost()
                                        .label("Cancel")
                                        .on_click(cx.listener(|this: &mut AppView, _: &ClickEvent, _window, cx| {
                                            this.cancel_confirm(cx);
                                        })),
                                )
                                .child(
                                    yes.label(confirm.yes.clone()).on_click(cx.listener(
                                        |this: &mut AppView, _: &ClickEvent, _window, cx| {
                                            this.confirm_yes(cx);
                                        },
                                    )),
                                ),
                        ),
                )
                .into_any_element(),
        )
    }
}
