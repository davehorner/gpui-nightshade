//! Opt-in native window lifecycle acceptance driver.
use super::*;

pub(super) fn start(window: &Window, cx: &mut Context<Editor>) {
    if std::env::var_os("GPUI_NIGHTSHADE_GPU_STRESS").is_some() {
        let handle = window.window_handle();
        cx.spawn(async move |view, cx| {
            cx.background_executor().timer(Duration::from_secs(5)).await;
            for step in 0..24 {
                cx.background_executor()
                    .timer(Duration::from_millis(600))
                    .await;
                if handle
                    .update(cx, |_, window, cx| {
                        let (width, height) =
                            [(1024., 680.), (1450., 900.), (800., 600.), (1280., 720.)][step % 4];
                        window.resize(size(px(width), px(height)));
                        if step % 8 == 7 {
                            let _ = view.update(cx, |view, cx| view.dock.reset(window, cx));
                        }
                    })
                    .is_err()
                {
                    return;
                }
                if step % 6 == 5 {
                    if handle
                        .update(cx, |_, window, _| window.minimize_window())
                        .is_err()
                    {
                        return;
                    }
                    cx.background_executor().timer(Duration::from_secs(1)).await;
                    if handle
                        .update(cx, |_, window, _| window.activate_window())
                        .is_err()
                    {
                        return;
                    }
                }
                eprintln!("Shared GPU lifecycle: step={}", step + 1);
            }
            eprintln!("Shared GPU lifecycle: complete");
        })
        .detach();
    }
}
