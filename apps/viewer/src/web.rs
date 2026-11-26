use gpui_kit::*;
use std::{borrow::Cow, cell::RefCell};
use wasm_bindgen::prelude::*;
gpui_kit::assets::icon_assets!(
    WebAssets,
    [
        ArrowDown,
        ArrowUp,
        Asterisk,
        Ban,
        BookOpen,
        Calendar,
        CaseSensitive,
        Check,
        ChevronDown,
        ChevronLeft,
        ChevronRight,
        ChevronUp,
        ChevronsUpDown,
        CircleCheck,
        CircleX,
        Close,
        Copy,
        Ellipsis,
        ExternalLink,
        Eye,
        EyeOff,
        Github,
        Inbox,
        Info,
        Inspector,
        Loader,
        Maximize,
        Mic,
        Minimize,
        Minus,
        PanelBottom,
        PanelBottomOpen,
        PanelLeft,
        PanelLeftClose,
        PanelLeftOpen,
        PanelRight,
        PanelRightClose,
        PanelRightOpen,
        Plus,
        RefreshCw,
        Replace,
        Search,
        SortAscending,
        SortDescending,
        Square,
        Star,
        StarFill,
        TriangleAlert,
        Undo2,
        User,
        WindowClose,
        WindowMaximize,
        WindowMinimize,
        WindowRestore
    ]
);
thread_local! {static APPLICATION:RefCell<Option<ApplicationHandle>>=const {RefCell::new(None)};
static PANIC:RefCell<String>=const {RefCell::new(String::new())};}
#[wasm_bindgen]
pub fn run(model: Option<String>) -> Result<(), JsValue> {
    std::panic::set_hook(Box::new(|info| {
        PANIC.with(|slot| *slot.borrow_mut() = info.to_string());
        console_error_panic_hook::hook(info);
    }));
    let _ = console_log::init_with_level(log::Level::Info);
    platform::web_init();
    let application = platform::single_threaded_web().with_assets(WebAssets);
    let handle=application.run_embedded(move |cx| {
  cx.text_system().add_fonts(vec![
                Cow::Borrowed(include_bytes!("../../../upstream/gpui-component/crates/story-web/fonts/IBMPlexSans-Regular.ttf").as_slice()),Cow::Borrowed(include_bytes!("../../../upstream/gpui-component/crates/story-web/fonts/Inter-Regular.ttf").as_slice()),Cow::Borrowed(include_bytes!("../../../upstream/gpui-component/crates/story-web/fonts/JetBrainsMono-Regular.ttf").as_slice())]).expect("bundled fonts");
  crate::editor::open_editor(cx,gpui_nightshade::presentation::BackendPreference::Readback,nightshade_viewer_core::ViewerArguments{model,no_agent:true,..Default::default()},gpui_nightshade_components::CameraLayout::Single);
 });
    APPLICATION.with(|slot| *slot.borrow_mut() = Some(handle));
    Ok(())
}

#[wasm_bindgen]
pub fn last_error() -> String {
    PANIC.with(|slot| slot.borrow().clone())
}
