mod launcher;

use tauri::{Manager, RunEvent, WebviewUrl, WebviewWindowBuilder};

fn main() {
    let app = tauri::Builder::default()
        .setup(move |app| {
            let base_url = match launcher::bootstrap(app.path().resource_dir().ok()) {
                Ok(bootstrap) => bootstrap.base_url,
                Err(err) => {
                    eprintln!("launcher bootstrap failed: {err}");
                    launcher::fallback_base_url()
                }
            };
            let external_url = url::Url::parse(&base_url)?;

            if let Some(window) = app.get_webview_window("main") {
                window.navigate(external_url)?;
                window.set_title("Suwayomi")?;
            } else {
                WebviewWindowBuilder::new(app, "main", WebviewUrl::External(external_url))
                    .title("Suwayomi")
                    .build()?;
            }
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("failed to build Tauri application");

    app.run(|_, event| {
        if matches!(event, RunEvent::Exit | RunEvent::ExitRequested { .. }) {
            launcher::shutdown_child_process();
        }
    });
}
