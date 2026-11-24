//! Squad: a system-tray icon for the background service on Windows, so the uploader can be
//! reached after its window is closed. Left-click opens the status window; right-click shows
//! a menu to open the window, update now, or open Soul Scribe.
//!
//! The icon lives on its own thread, which runs the Win32 message loop the icon needs.

#[cfg(windows)]
pub(crate) fn spawn() {
    let spawned = std::thread::Builder::new()
        .name("tray".into())
        .spawn(windows::run);
    if let Err(e) = spawned {
        tracing::warn!("tray: failed to start: {e}");
    }
}

#[cfg(not(windows))]
pub(crate) fn spawn() {}

#[cfg(windows)]
mod windows {
    use tracing::{info, warn};
    use tray_icon::menu::{Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem};
    use tray_icon::{
        Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent,
    };

    use crate::service::{gui_open, quiet, spawn_detached};
    use crate::{console, paths, update};

    const SITE_URL: &str = "https://deadlock.andrewtkeezer.workers.dev/";

    struct Items {
        open: MenuId,
        update: MenuId,
        site: MenuId,
    }

    pub(super) fn run() {
        // `_tray` must stay alive for the icon to stay visible.
        let Some((_tray, items)) = build() else {
            return;
        };
        message_loop(&items);
    }

    /// The icon and its menu.
    fn build() -> Option<(TrayIcon, Items)> {
        const RGBA: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/window-icon.rgba"));
        let icon = match Icon::from_rgba(RGBA.to_vec(), 64, 64) {
            Ok(icon) => icon,
            Err(e) => {
                warn!("tray: invalid icon: {e}");
                return None;
            }
        };
        let open = MenuItem::new("Open Soul Scribe uploader", true, None);
        let update = MenuItem::new("Update now", true, None);
        let site = MenuItem::new("Open Soul Scribe", true, None);
        let menu = Menu::new();
        if let Err(e) =
            menu.append_items(&[&open, &update, &PredefinedMenuItem::separator(), &site])
        {
            warn!("tray: cannot build the menu: {e}");
            return None;
        }
        let tray = TrayIconBuilder::new()
            .with_icon(icon)
            .with_tooltip(format!(
                "Soul Scribe uploader v{}",
                env!("CARGO_PKG_VERSION")
            ))
            .with_menu(Box::new(menu))
            .with_menu_on_left_click(false)
            .build();
        match tray {
            Ok(tray) => {
                info!("tray: icon added");
                let items = Items {
                    open: open.id().clone(),
                    update: update.id().clone(),
                    site: site.id().clone(),
                };
                Some((tray, items))
            }
            Err(e) => {
                warn!("tray: cannot add the icon: {e}");
                None
            }
        }
    }

    /// Pumps Win32 messages for the icon's hidden window, handling clicks as they arrive.
    #[allow(unsafe_code)]
    fn message_loop(items: &Items) {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            DispatchMessageW, GetMessageW, MSG, TranslateMessage,
        };

        // SAFETY: MSG is a plain C struct; all-zero is a valid value that GetMessageW overwrites.
        let mut msg: MSG = unsafe { core::mem::zeroed() };
        loop {
            // SAFETY: `msg` is valid for writes; a null window handle means any window of
            // this thread. Returns 0 on WM_QUIT and -1 on error, both end the loop.
            let got = unsafe { GetMessageW(&raw mut msg, core::ptr::null_mut(), 0, 0) };
            if got <= 0 {
                break;
            }
            // SAFETY: `msg` was just filled in by GetMessageW.
            unsafe {
                TranslateMessage(&raw const msg);
                DispatchMessageW(&raw const msg);
            }
            while let Ok(event) = TrayIconEvent::receiver().try_recv() {
                if let TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                } = event
                {
                    open_window();
                }
            }
            while let Ok(event) = MenuEvent::receiver().try_recv() {
                if event.id == items.open {
                    open_window();
                } else if event.id == items.update {
                    update_now();
                } else if event.id == items.site {
                    open_site();
                }
            }
        }
    }

    fn open_window() {
        if gui_open() {
            return;
        }
        let result = paths::current_exe().and_then(|exe| spawn_detached(&exe, &["--gui"]));
        if let Err(e) = result {
            warn!("tray: cannot open the window: {e}");
        }
    }

    /// Checks and installs on a worker thread, so the menu stays responsive. The installer
    /// stops this process and starts the new version, which adds its own icon.
    fn update_now() {
        let spawned = std::thread::Builder::new()
            .name("tray-update".into())
            .spawn(|| match update::check() {
                Ok(Some(found)) => {
                    info!("tray: installing version {}", found.version);
                    if let Err(e) = update::apply_silent(&found) {
                        console::message_box(&format!("Update failed:\n\n{e}"), true);
                    }
                }
                Ok(None) => console::message_box(
                    &format!("You're up to date (version {}).", env!("CARGO_PKG_VERSION")),
                    false,
                ),
                Err(e) => console::message_box(&e, true),
            });
        if let Err(e) = spawned {
            warn!("tray: cannot start the update: {e}");
        }
    }

    fn open_site() {
        if let Err(e) = quiet("explorer").arg(SITE_URL).spawn() {
            warn!("tray: cannot open the site: {e}");
        }
    }
}
