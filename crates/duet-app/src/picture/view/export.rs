//! Export difference image (R13, `js:689-713`): the difference as a PNG. The image is rebuilt
//! from the pictures and tolerance the shown diff was made from and written off the UI
//! thread, after the save dialog.

use duet_core::picture::export::{encode_png, export_name};

use super::*;
use crate::paths::home_dir;

impl PictureView {
    /// Asks where to save, then writes the difference image there.
    pub(super) fn export(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(compared) = self.compared.as_ref().filter(|_| self.can_export()) else {
            return;
        };
        let (sides, tolerance) = (compared.sides.clone(), compared.tolerance);
        let (Some(left), Some(right)) = (self.side(true), self.side(false)) else {
            return;
        };
        let file = export_name(&left.name, &right.name);
        let dir = left
            .path
            .as_deref()
            .and_then(Path::parent)
            .filter(|dir| !dir.as_os_str().is_empty())
            .map_or_else(home_dir, Path::to_path_buf);
        let picked = cx.prompt_for_new_path(&dir, Some(&file));
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(path))) = picked.await else {
                return;
            };
            let target = path.clone();
            let written = cx.background_executor().spawn(async move {
                let image = diff_image(&sides[LEFT], &sides[RIGHT], tolerance, &CancelToken::new())
                    .map_err(|_| "cancelled".to_owned())?;
                let png = encode_png(&image)?;
                std::fs::write(&target, png).map_err(|err| err.to_string())
            });
            let result = written.await;
            let _ = this.update(cx, |view, cx| {
                view.notice = Some(
                    match result {
                        Ok(()) => format!("Exported: {}.", path.display()),
                        Err(err) => format!("Can't export: {err}."),
                    }
                    .into(),
                );
                cx.notify();
            });
        })
        .detach();
    }
}
