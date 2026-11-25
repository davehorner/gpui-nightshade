mod demo;
mod editor;
mod thumbnails;
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let backend = args
        .iter()
        .find_map(|arg| arg.strip_prefix("--presentation="))
        .map(str::to_string)
        .or_else(|| std::env::var("GPUI_NIGHTSHADE_PRESENTATION").ok())
        .unwrap_or_else(|| "auto".into())
        .parse()
        .expect("use --presentation=auto|readback|windows-shared|windows-shared-auto");
    if args
        .iter()
        .any(|arg| arg == "--cube" || arg == "--multi-viewport")
    {
        demo::launch(backend, args.iter().any(|arg| arg == "--multi-viewport"));
        return;
    }
    let arguments = nightshade_viewer_core::ViewerArguments {
        model: args
            .iter()
            .find_map(|arg| arg.strip_prefix("--model="))
            .map(str::to_string),
        no_agent: args.iter().any(|arg| arg == "--no-agent"),
        agent_port: args
            .iter()
            .find_map(|arg| arg.strip_prefix("--agent-port="))
            .map(|v| v.parse().expect("invalid agent port")),
        ..Default::default()
    };
    let camera_layout = match args
        .iter()
        .find_map(|arg| arg.strip_prefix("--camera-views="))
    {
        None | Some("1") => gpui_nightshade_components::CameraLayout::Single,
        Some("2") => gpui_nightshade_components::CameraLayout::Split,
        Some("4") => gpui_nightshade_components::CameraLayout::Quad,
        _ => panic!("use --camera-views=1|2|4"),
    };
    editor::launch(backend, arguments, camera_layout);
}
