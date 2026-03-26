// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![allow(warnings)]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // ------------------------------------------------------------------------
    // HACK: Hook Mode - Are we launched by VS Code as the language server?
    // ------------------------------------------------------------------------
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--lsp_port" || a == "--parent_pipe_path") {
        if let Ok(exe_path) = std::env::current_exe() {
            if let Some(bin_dir) = exe_path.parent() {
                // We are running as the hook inside the VS Code extensions folder
                let rt = tokio::runtime::Runtime::new().unwrap();
                
                let res = rt.block_on(antigravity_tools_lib::proxy::launcher::launch_language_server(
                    args, 
                    bin_dir.to_path_buf()
                ));
                
                if let Err(e) = res {
                    eprintln!("[HOOK ERROR] {}", e);
                    std::process::exit(1);
                }
                std::process::exit(0);
            }
        }
    }
    // ------------------------------------------------------------------------

    #[cfg(target_os = "linux")]
    {
        // Fix for transparent window on some Linux systems
        // See: https://github.com/spacedriveapp/spacedrive/issues/1512#issuecomment-1758550164
        std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
    }

    antigravity_tools_lib::run()
}
