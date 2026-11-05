//! The status bar (R17): what the compare says, the file warnings and the colour legend.

use super::*;
use crate::i18n::tr;
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
            (Some(notice), _, _) => status_notice("text-status-text", notice.clone(), palette),
            (None, true, _) => status_line_box(
                "text-status-text",
                tr("Comparing… Press Esc to cancel."),
                tr("Comparing… Press Esc to cancel."),
            ),
            (None, false, Some(model)) => {
                status_parts_row("text-status-text", &status_parts(model), palette)
            }
            (None, false, None) => status_line_box("text-status-text", "", ""),
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
