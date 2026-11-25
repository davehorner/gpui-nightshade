mod demo;
mod editor;
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let demo = args
        .iter()
        .any(|arg| arg == "--cube" || arg == "--multi-viewport");
    let backend = args
        .iter()
        .find_map(|arg| arg.strip_prefix("--presentation="))
        .map(str::to_string)
        .or_else(|| std::env::var("GPUI_NIGHTSHADE_PRESENTATION").ok())
        .unwrap_or_else(|| {
            if demo {
                "auto".into()
            } else {
                "readback".into()
            }
        })
        .parse()
        .expect("use --presentation=auto|readback|windows-shared");
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
    editor::launch(backend, arguments);
}
