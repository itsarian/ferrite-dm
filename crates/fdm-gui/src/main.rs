mod app;
mod assets;
mod bottom_panel;
mod helpers;
mod i18n;
mod menus;
mod modals;
mod plugins_page;
mod qr_view;
mod settings_page;
mod standalone;
mod text_input;
mod theme;
mod ui_widgets;

use app::{AppWindowGlobal, FerriteApp, bind_app_keys_and_menus};
use assets::FerriteAssets;
use helpers::{detect_lan_ip, is_portable_active, macos_restore_windows_and_activate, parse_launch_args};
use text_input::bind_text_input_keys;

use fdm_cli::DownloadEngine;
use fdm_core::FdmConfig;
use gpui::{
    App, Application, Bounds, TitlebarOptions, WindowBounds, WindowOptions, prelude::*, px, size,
};
use std::sync::Arc;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let launch = parse_launch_args(&args);
    let portable = is_portable_active(launch.portable);

    let data_dir = launch
        .data_dir
        .clone()
        .unwrap_or_else(|| FdmConfig::data_dir(portable));

    let rt = Arc::new(
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("Failed to create Tokio runtime"),
    );

    let engine = rt
        .block_on(async {
            let eng = DownloadEngine::open(&data_dir, false)?;
            let _sup = eng.spawn_supervisor();
            if let Err(e) = eng.start_ipc_listener().await {
                eprintln!("[ferrite-gui] IPC listener note: {e}");
            }
            if eng.get_config().app.enable_remote_access {
                let eng_rc = Arc::clone(&eng);
                let host = detect_lan_ip();
                tokio::spawn(async move {
                    let _ = eng_rc.ensure_remote_control(&host).await;
                });
            }
            Ok::<_, anyhow::Error>(eng)
        })
        .expect("Failed to initialize Ferrite DownloadEngine");

    let launch_for_app = launch.clone();
    let rt_for_app = Arc::clone(&rt);
    let eng_for_app = Arc::clone(&engine);

    let gpui_app = Application::new().with_assets(FerriteAssets);
    gpui_app.on_reopen(|cx| {
        macos_restore_windows_and_activate();
        cx.activate(true);
    });
    gpui_app.run(move |cx: &mut App| {
        bind_text_input_keys(cx);
        bind_app_keys_and_menus(cx);

        cx.on_window_closed(|cx| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();

        let bounds = Bounds::centered(None, size(px(1220.0), px(808.0)), cx);
        let opts = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(TitlebarOptions {
                title: Some("Ferrite DM".into()),
                appears_transparent: true,
                traffic_light_position: Some(gpui::point(px(12.), px(8.))),
            }),
            show: !launch_for_app.hidden,
            ..Default::default()
        };

        let rt_clone = Arc::clone(&rt_for_app);
        let eng_clone = Arc::clone(&eng_for_app);
        let launch_clone = launch_for_app.clone();

        let win_handle = cx
            .open_window(opts, move |window, cx| {
                cx.new(|cx| {
                    FerriteApp::new(rt_clone, eng_clone, launch_clone, window, cx)
                })
            })
            .expect("Failed to open Ferrite DM main window");

        if let Some(view_entity) = win_handle.entity(cx).ok() {
            cx.set_global(AppWindowGlobal {
                entity: view_entity.downgrade(),
                window: win_handle,
            });
        }

        if !launch_for_app.hidden {
            cx.activate(true);
        }
    });
}
