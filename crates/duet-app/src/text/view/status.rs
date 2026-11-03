//! The status bar (R17): what the compare says, the file warnings and the colour legend.

use super::*;
use crate::text::warnings::warnings_text;

impl TextView {
    /// The warnings about the two files as they are; None when they agree.
    pub fn warnings(&self) -> Option<String> {
        let model = self.model.as_ref()?;
        warnings_text(model.left(), model.right())
    }

    pub(super) fn render_status(
        &self,
        palette: &Palette,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let left = match (&self.notice, self.job.is_running(), &self.model) {
            (Some(notice), _, _) => div().child(notice.clone()).text_color(palette.text),
            (None, true, _) => div().child("Comparing… Press Esc to cancel."),
            (None, false, Some(model)) => status_parts_row(&status_parts(model), palette),
            (None, false, None) => div(),
        };
        let warnings = self.warnings().map(|text| {
            div()
                .id("text-warnings")
                .test_support()
                .text_color(palette.modified)
                .child(text)
        });
        status_bar("text-status", palette)
            .child(left)
            .children(warnings)
            .children(self.render_odd(palette, cx))
            .when(
                window.viewport_size().width >= px(LEGEND_MIN_WIDTH),
                |bar| bar.child(legend("text-legend", palette)),
            )
    }
}
