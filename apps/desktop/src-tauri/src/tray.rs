//! System tray: open, per-server start/stop, quit. The menu is rebuilt when servers
//! change state.

use crate::commands::show_main;
use crate::state::AppState;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, Wry};

pub const TRAY_ID: &str = "mcpanel-tray";

async fn build_menu(app: &AppHandle) -> tauri::Result<Menu<Wry>> {
    let menu = Menu::new(app)?;
    menu.append(&MenuItem::with_id(
        app,
        "open",
        "Open MCPanel",
        true,
        None::<&str>,
    )?)?;
    menu.append(&PredefinedMenuItem::separator(app)?)?;
    let state = app.state::<AppState>();
    match state.api.servers_list(&state.principal()).await {
        Ok(servers) if !servers.is_empty() => {
            for s in servers.iter().take(15) {
                let running = matches!(s.state.as_str(), "starting" | "running");
                let (id, label, enabled) = if running {
                    (format!("stop:{}", s.id), format!("Stop {}", s.name), true)
                } else if s.state == "stopping" || s.state == "restarting" {
                    (
                        format!("noop:{}", s.id),
                        format!("{} ({})", s.name, s.state),
                        false,
                    )
                } else if s.state == "detached" {
                    (
                        format!("noop:{}", s.id),
                        format!("{} (detached)", s.name),
                        false,
                    )
                } else {
                    (format!("start:{}", s.id), format!("Start {}", s.name), true)
                };
                menu.append(&MenuItem::with_id(app, id, label, enabled, None::<&str>)?)?;
            }
            menu.append(&PredefinedMenuItem::separator(app)?)?;
        }
        _ => {}
    }
    menu.append(&MenuItem::with_id(
        app,
        "quit",
        "Quit MCPanel",
        true,
        None::<&str>,
    )?)?;
    Ok(menu)
}

pub async fn refresh(app: &AppHandle) {
    match build_menu(app).await {
        Ok(menu) => {
            if let Some(tray) = app.tray_by_id(TRAY_ID) {
                let _ = tray.set_menu(Some(menu));
            }
        }
        Err(e) => tracing::warn!("cannot rebuild tray menu: {e}"),
    }
}

/// Quit request from the tray: ask the UI when servers are running.
pub fn request_quit(app: &AppHandle) {
    let state = app.state::<AppState>();
    let running = state
        .api
        .servers_running_count(&state.principal())
        .unwrap_or(0);
    if running == 0 {
        state
            .quitting
            .store(true, std::sync::atomic::Ordering::SeqCst);
        app.exit(0);
    } else {
        show_main(app);
        let _ = app.emit("mcpanel://quit-requested", running);
    }
}

pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let menu = Menu::new(app)?;
    menu.append(&MenuItem::with_id(
        app,
        "open",
        "Open MCPanel",
        true,
        None::<&str>,
    )?)?;
    menu.append(&MenuItem::with_id(
        app,
        "quit",
        "Quit MCPanel",
        true,
        None::<&str>,
    )?)?;
    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip("MCPanel")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| {
            let id = event.id().as_ref().to_string();
            let app = app.clone();
            match id.as_str() {
                "open" => show_main(&app),
                "quit" => request_quit(&app),
                other => {
                    if let Some((action, server)) = other.split_once(':') {
                        let action = action.to_string();
                        let server = server.to_string();
                        tauri::async_runtime::spawn(async move {
                            let state = app.state::<AppState>();
                            let p = state.principal();
                            let r = match action.as_str() {
                                "start" => state.api.servers_start(&p, &server).await,
                                "stop" => state.api.servers_stop(&p, &server, false).await,
                                _ => Ok(()),
                            };
                            if let Err(e) = r {
                                let _ = app.emit("mcpanel://notice", e.message.clone());
                                show_main(&app);
                            }
                        });
                    }
                }
            }
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}
