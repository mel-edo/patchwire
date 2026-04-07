use std::sync::{Arc, Mutex};
use gtk4::prelude::*;
use gtk4::{Align, ScrolledWindow, PolicyType, Box as GBox, Orientation, glib};
use libadwaita as adw;
use adw::prelude::*;
use tokio::runtime::Handle;

use crate::dbus_client::{PatchwireDaemonProxy, SinkInfo};

pub struct PatchwireWindow {
    pub window: adw::ApplicationWindow,
}

impl PatchwireWindow {
    pub fn new(
        app: &adw::Application,
        proxy: PatchwireDaemonProxy<'static>,
        rt: Handle,
    ) -> Self {
        let proxy = Arc::new(proxy);
        // root window
        let window = adw::ApplicationWindow::builder()
            .application(app)
            .title("Patchwire")
            .default_width(420)
            .default_height(560)
            .build();

        // toolbar view
        let toolbar_view = adw::ToolbarView::new();
        window.set_content(Some(&toolbar_view));

        // header bar
        let header = adw::HeaderBar::new();
        header.set_decoration_layout(Some(":close"));
        header.add_css_class("flat");
        toolbar_view.add_top_bar(&header);

        // scrolled + clamp
        let scroll = ScrolledWindow::builder()
            .hscrollbar_policy(PolicyType::Never)
            .vscrollbar_policy(PolicyType::Automatic)
            .build();
        toolbar_view.set_content(Some(&scroll));

        let clamp = adw::Clamp::builder()
            .maximum_size(600)
            .build();
        scroll.set_child(Some(&clamp));

        let outer_box = GBox::builder()
            .orientation(Orientation::Vertical)
            .spacing(12)
            .margin_top(24)
            .margin_bottom(12)
            .margin_start(12)
            .margin_end(12)
            .build();
        clamp.set_child(Some(&outer_box));

        // sinks group
        let sink_group = adw::PreferencesGroup::new();
        sink_group.set_title("Audio Sinks");
        sink_group.set_description(Some("Toggle routing to secondary outputs"));
        outer_box.append(&sink_group);

        // default sink expander
        let default_expander = adw::ExpanderRow::new();
        default_expander.set_title("Default sink");
        default_expander.set_subtitle("Loading...");
        let icon = gtk4::Image::from_icon_name("audio-volume-high-symbolic");
        default_expander.add_prefix(&icon);
        let default_change_btn = gtk4::Button::with_label("Change");
        default_change_btn.set_valign(Align::Center);
        default_change_btn.add_css_class("flat");
        default_expander.add_suffix(&default_change_btn);

        // default volume slider inside expander
        let default_vol_row = adw::ActionRow::new();
        default_vol_row.set_title("Volume");
        let default_scale = gtk4::Scale::with_range(Orientation::Horizontal, 0.0, 1.0, 0.01);
        default_scale.set_hexpand(true);
        default_scale.set_valign(Align::Center);
        default_scale.set_width_request(200);
        default_scale.set_draw_value(true);
        default_scale.set_format_value_func(|_, v| format!("{:.0}%", v * 100.0));
        default_vol_row.add_suffix(&default_scale);
        default_expander.add_row(&default_vol_row);
        sink_group.add(&default_expander);

        // shared state - store sink rows so we can update them from singal callbacks
        let sink_rows: Arc<Mutex<Vec<(String, gtk4::Switch, adw::ExpanderRow)>>> =
            Arc::new(Mutex::new(Vec::new()));
        let handler_ids: Arc<Mutex<Vec<(String, glib::SignalHandlerId)>>> =
            Arc::new(Mutex::new(Vec::new()));

        // initial data fetch
        {
            let sinks = rt.block_on(async {
                for _ in 0..10 {
                    let sinks = proxy.list_sinks().await.unwrap_or_default();
                    if !sinks.is_empty() {
                        return sinks;
                    }
                    eprintln!("no sinks yet, retrying...");
                    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
                }
                eprintln!("giving up waiting for sinks");
                vec![]
            });

            // wire default sink volume slider
            if let Some(default_sink) = sinks.iter().find(|s| s.is_default) {
                let vol = rt.block_on(async {
                    proxy.get_sink_volume(&default_sink.name).await.unwrap_or(1.0)
                });
                default_scale.set_value(vol);

                let name = default_sink.name.clone();
                let proxy_vol = Arc::clone(&proxy);
                let rt_vol = rt.clone();
                default_scale.connect_value_changed(move |s| {
                    let vol = s.value() as f32;
                    let name = name.clone();
                    let proxy2 = Arc::clone(&proxy_vol);
                    rt_vol.spawn(async move {
                        if let Err(e) = proxy2.set_sink_volume(&name, vol).await {
                            eprintln!("set_sink_volume failed: {e}");
                        }
                    });
                });
            }

            populate_sinks(
                &sink_group,
                &default_expander,
                &sink_rows,
                &handler_ids,
                sinks,
                Arc::clone(&proxy),
                rt.clone(),
            );
        }

        // signal subscriptions
        {
            let proxy_for_signal = Arc::clone(&proxy);
            let proxy_for_default_signal = Arc::clone(&proxy);
            let sink_rows = Arc::clone(&sink_rows);
            let sink_rows_for_default = Arc::clone(&sink_rows);
            let handler_ids_for_default = Arc::clone(&handler_ids);
            let sink_group_for_default = sink_group.clone();
            let default_expander_for_default = default_expander.clone();

            // channel carries plain Vec<SinkInfo> from tokio to glib main context
            let (tx, rx) = std::sync::mpsc::channel::<Vec<crate::dbus_client::SinkInfo>>();
            let tx_for_default = tx.clone();

            // listen for SinksChanged
            rt.spawn(async move {
                use futures_util::StreamExt;
                if let Ok(mut stream) = proxy_for_signal.receive_sinks_changed().await {
                    while stream.next().await.is_some() {
                        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                        let sinks = proxy_for_signal.list_sinks().await.unwrap_or_default();
                        let _ = tx.send(sinks);
                    }
                }
            });

            // listen for DefaultChanged
            let proxy_for_default2 = Arc::clone(&proxy_for_default_signal);
            rt.spawn(async move {
                use futures_util::StreamExt;
                if let Ok(mut stream) = proxy_for_default2.receive_default_changed().await {
                    while stream.next().await.is_some() {
                        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
                        let sinks = proxy_for_default2.list_sinks().await.unwrap_or_default();
                        let _ = tx_for_default.send(sinks);
                    }
                }
            });

            let proxy_for_poll = Arc::clone(&proxy);
            let rt_for_poll = rt.clone();
            glib::timeout_add_local(std::time::Duration::from_millis(100), move || {
                while let Ok(sinks) = rx.try_recv() {
                    populate_sinks(
                        &sink_group_for_default,
                        &default_expander_for_default,
                        &sink_rows_for_default,
                        &handler_ids_for_default,
                        sinks,
                        Arc::clone(&proxy_for_poll),
                        rt_for_poll.clone(),
                    );
                }
                glib::ControlFlow::Continue
            });

            let handler_ids_for_state = Arc::clone(&handler_ids);
            let _ = handler_ids_for_state;
        }

        // default sink popover
        {
            let proxy = Arc::clone(&proxy);
            let rt2 = rt.clone();
            let default_expander = default_expander.clone();

            let popover = gtk4::Popover::new();
            popover.set_parent(&default_change_btn);

            let pop_box = GBox::builder()
                .orientation(Orientation::Vertical)
                .spacing(4)
                .margin_top(8)
                .margin_bottom(8)
                .margin_start(8)
                .margin_end(8)
                .build();
            popover.set_child(Some(&pop_box));

            // fetch all sinks and build a button for each non default one
            let all_sinks: Arc<Mutex<Vec<SinkInfo>>> = Arc::new(Mutex::new(Vec::new()));

            {
                let all_sinks = Arc::clone(&all_sinks);
                let sinks = rt2.block_on(async {
                    proxy.list_sinks().await.unwrap_or_default()
                });
                *all_sinks.lock().unwrap() = sinks;
            }

            {
                let popover = popover.clone();
                let all_sinks = Arc::clone(&all_sinks);
                let proxy = Arc::clone(&proxy);
                let rt2 = rt.clone();
                let default_expander = default_expander.clone();
                let pop_box = pop_box.clone();

                default_change_btn.connect_clicked(move |_| {
                    // clear old button
                    while let Some(child) = pop_box.first_child() {
                        pop_box.remove(&child);
                    }

                    let sinks = all_sinks.lock().unwrap().clone();
                    for sink in sinks {
                        let btn = gtk4::Button::with_label(&sink.description);
                        btn.add_css_class("flat");
                        if sink.is_default {
                            btn.add_css_class("accent");
                        }

                        let proxy2 = Arc::clone(&proxy);
                        let rt3 = rt2.clone();
                        let popover2 = popover.clone();
                        let default_expander2 = default_expander.clone();
                        let desc = sink.description.clone();
                        let name = sink.name.clone();

                        btn.connect_clicked(move |_| {
                            popover2.popdown();
                            let proxy3 = Arc::clone(&proxy2);
                            let name2 = name.clone();
                            let desc2 = desc.clone();
                            let default_expander3 = default_expander2.clone();
                            let success = rt3.block_on(async move {
                                match proxy3.set_default_sink(&name2).await {
                                    Ok(_) => true,
                                    Err(e) => {
                                        eprintln!("set_default_sink failed: {e}");
                                        false
                                    }
                                }
                            });
                            if success {
                                default_expander3.set_subtitle(&desc2);
                            }
                        });
                        pop_box.append(&btn);
                    }
                    popover.popup();
                });
            }

            let (tx2, rx2) = std::sync::mpsc::channel::<Vec<SinkInfo>>();
            let proxy_for_default = Arc::clone(&proxy);
            rt.spawn(async move {
                use futures_util::StreamExt;
                if let Ok(mut steam) = proxy_for_default.receive_sinks_changed().await {
                    while steam.next().await.is_some() {
                        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                        let sinks = proxy_for_default.list_sinks().await.unwrap_or_default();
                        let _ = tx2.send(sinks);
                    }
                }
            });

            glib::timeout_add_local(std::time::Duration::from_millis(100), move || {
                while let Ok(sinks) = rx2.try_recv() {
                    *all_sinks.lock().unwrap() = sinks;
                }
                glib::ControlFlow::Continue
            });
        }

        // quick actions group
        let action_group = adw::PreferencesGroup::new();
        action_group.set_title("Quick Actions");
        outer_box.append(&action_group);

        let action_row = adw::ActionRow::new();
        action_row.set_title("Route audio");
        action_row.set_subtitle("Enable or disable all secondary outputs at once");

        let toggle_all_btn = gtk4::Button::with_label("All On");
        toggle_all_btn.set_valign(Align::Center);
        toggle_all_btn.add_css_class("suggested-action");

        action_row.add_suffix(&toggle_all_btn);
        action_group.add(&action_row);
        
        {
            let proxy = Arc::clone(&proxy);
            let rt2 = rt.clone();
            let sink_rows = Arc::clone(&sink_rows);
            let handler_ids = Arc::clone(&handler_ids);
            let all_on = {
                let rows = sink_rows.lock().unwrap();
                let any_enabled = rows.iter().any(|(_, toggle, _)| toggle.is_active());
                Arc::new(Mutex::new(any_enabled))
            };
            if *all_on.lock().unwrap() {
                toggle_all_btn.set_label("All Off");
                toggle_all_btn.remove_css_class("suggested-action");
                toggle_all_btn.add_css_class("destructive-action");
            }

            toggle_all_btn.connect_clicked(move |btn| {
                let mut state = all_on.lock().unwrap();
                *state = !*state;
                let enable = *state;

                if enable {
                    btn.set_label("All Off");
                    btn.remove_css_class("suggested-action");
                    btn.add_css_class("destructive-action");
                } else {
                    btn.set_label("All On");
                    btn.remove_css_class("destructive-action");
                    btn.add_css_class("suggested-action");
                }

                let rows = sink_rows.lock().unwrap();
                let ids = handler_ids.lock().unwrap();
                for (name, toggle, _) in rows.iter() {
                    if let Some((_, id)) = ids.iter().find(|(n, _)| n == name) {
                        toggle.block_signal(id);
                        toggle.set_active(enable);
                        toggle.unblock_signal(id);
                    }
                    let name = name.clone();
                    let proxy2 = Arc::clone(&proxy);
                    rt2.spawn(async move {
                        if let Err(e) = proxy2.set_sink_enabled(&name, enable).await {
                            eprintln!("set_sink_enabled failed: {e}");
                        }
                    });
                }
            });
        }

        Self { window }
    }

    pub fn present(&self) {
        self.window.present();
    }
}

// helpers
fn populate_sinks(
    sink_group: &adw::PreferencesGroup,
    default_expander: &adw::ExpanderRow,
    sink_rows: &Arc<Mutex<Vec<(String, gtk4::Switch, adw::ExpanderRow)>>>,
    handler_ids: &Arc<Mutex<Vec<(String, glib::SignalHandlerId)>>>,
    sinks: Vec<SinkInfo>,
    proxy: Arc<PatchwireDaemonProxy<'static>>,
    rt: Handle,
) {
    // clear old rows
    let mut rows = sink_rows.lock().unwrap();
    let mut ids = handler_ids.lock().unwrap();

    for (_, _, expander) in rows.iter() {
        sink_group.remove(expander);
    }
    rows.clear();
    ids.clear();

    for sink in sinks {
        if sink.is_default {
            default_expander.set_subtitle(&sink.description);
            continue;
        }

        // expander row (the sink header)
        let expander = adw::ExpanderRow::new();
        expander.set_title(&sink.description);
        expander.set_subtitle(&sink.name);

        // toggle switch on the right
        let toggle = gtk4::Switch::new();
        toggle.set_valign(Align::Center);
        expander.add_suffix(&toggle);

        // wire toggle
        let name = sink.name.clone();
        let proxy2 = Arc::clone(&proxy);
        let rt2 = rt.clone();
        let handler_id = toggle.connect_active_notify(move |t| {
            let enabled = t.is_active();
            let name = name.clone();
            let proxy3 = Arc::clone(&proxy2);
            rt2.spawn(async move {
                if let Err(e) = proxy3.set_sink_enabled(&name, enabled).await {
                    eprintln!("set_sink_enabled failed: {e}");
                }
            });
        });

        toggle.block_signal(&handler_id);
        toggle.set_active(sink.is_enabled);
        toggle.unblock_signal(&handler_id);

        // volume row inside the expander
        let vol_row = adw::ActionRow::new();
        vol_row.set_title("Volume");

        let scale = gtk4::Scale::with_range(Orientation::Horizontal, 0.0, 1.0, 0.01);
        scale.set_hexpand(true);
        scale.set_valign(Align::Center);
        scale.set_width_request(200);
        scale.set_draw_value(true);
        scale.set_format_value_func(|_, v| format!("{:.0}%", v * 100.0));

        // fetch current volume
        let current_vol = rt.block_on(async {
            proxy.get_sink_volume(&sink.name).await.unwrap_or(1.0)
        });
        scale.set_value(current_vol);

        // wire volume change - use value_changed with a small debounce flag
        let name_vol = sink.name.clone();
        let proxy_vol = Arc::clone(&proxy);
        let rt_vol = rt.clone();
        scale.connect_value_changed(move |s| {
            let vol = s.value() as f32;
            let name = name_vol.clone();
            let proxy2 = Arc::clone(&proxy_vol);
            rt_vol.spawn(async move {
                if let Err(e) = proxy2.set_sink_volume(&name, vol).await {
                    eprintln!("set_sink_volume failed: {e}");
                }
            });
        });

        vol_row.add_suffix(&scale);
        expander.add_row(&vol_row);
        ids.push((sink.name.clone(), handler_id));
        rows.push((sink.name.clone(), toggle, expander.clone()));
        sink_group.add(&expander);
    }
}